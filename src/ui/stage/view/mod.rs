//! The stage's view: everything that takes a painter.
//!
//! Moved here verbatim from `stage/mod.rs` on 2026-09-05 as the first
//! step of the UI seam (`notes/20260905-ui-seam-contract.md`). The core
//! in the parent module names no egui; this module is where the pixels
//! are. A child module on purpose: it reads the core's private state
//! and the core cannot read back into it.

mod arrangement;
mod faces;
mod input;
mod mixer;
mod ornament;
mod scenes;
mod strip;
mod trig_menu;
mod utility;

use super::key::{Key, Mods};
use super::*;
use eframe::egui;

/// The fixed casing under every aperture. In the house-dark projection it
/// is true black, below even the field ground; in daylight it remains a
/// darker metal around the paper-bright displays.
fn shell_base(polarity: design::Polarity) -> egui::Color32 {
    match polarity {
        design::Polarity::Dark => design::chrome(0),
        design::Polarity::Light => design::chrome(150),
    }
}

/// The raised plates bolted to the casing. Still below ordinary cards and
/// display faces, but far enough from the base for the shell to have depth.
fn shell_plate(polarity: design::Polarity) -> egui::Color32 {
    match polarity {
        design::Polarity::Dark => design::chrome(32),
        design::Polarity::Light => design::chrome(190),
    }
}

/// The corner the deck gives up when it IS the screen. Windowed, the
/// compositor owns the corners and the frame keeps its bevels at the
/// house chamfer; fullscreen, the top left is cut at this size, so the
/// casing reads as one made object rather than a picture that happens
/// to fill the display. One corner, not four: a bevel repeated is a
/// border, a bevel once is a mark. Larger than the periphery is tall, so
/// the cut is seen to pass through the vitals strip.
const SCREEN_CHAMFER: f32 = 40.0;

/// The foot's chamfers, one each side, at a fraction of the crown's.
/// Much smaller on purpose: the top left is the mark, and the two below
/// are the casing agreeing with it — the same hand, not the same word.
/// The top right stays square so the three cuts read as a deliberate
/// asymmetry rather than as a template stamped on every corner.
const SCREEN_CHAMFER_FOOT: f32 = 14.0;

/// Which corner of the screen a cut is on. The top right is not here:
/// see `SCREEN_CHAMFER_FOOT`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScreenCorner {
    TopLeft,
    BottomLeft,
    BottomRight,
}

impl ScreenCorner {
    const ALL: [ScreenCorner; 3] = [
        ScreenCorner::TopLeft,
        ScreenCorner::BottomLeft,
        ScreenCorner::BottomRight,
    ];

    fn size(self) -> f32 {
        match self {
            ScreenCorner::TopLeft => SCREEN_CHAMFER,
            ScreenCorner::BottomLeft | ScreenCorner::BottomRight => SCREEN_CHAMFER_FOOT,
        }
    }
}

/// A cut: a right triangle on one corner of `whole`. The first point is
/// the corner itself; the other two lie along the window's edges, the
/// face between them. Never larger than the window can hold.
fn screen_cut(whole: egui::Rect, corner: ScreenCorner) -> [egui::Pos2; 3] {
    let c = corner.size().min(whole.width()).min(whole.height());
    let (l, t, r, b) = (whole.min.x, whole.min.y, whole.max.x, whole.max.y);
    match corner {
        ScreenCorner::TopLeft => [egui::pos2(l, t), egui::pos2(l + c, t), egui::pos2(l, t + c)],
        ScreenCorner::BottomLeft => [egui::pos2(l, b), egui::pos2(l + c, b), egui::pos2(l, b - c)],
        ScreenCorner::BottomRight => [egui::pos2(r, b), egui::pos2(r - c, b), egui::pos2(r, b - c)],
    }
}

/// The crown's cut: the top-left corner, which the rings live in.
fn screen_chamfer_cut(whole: egui::Rect) -> [egui::Pos2; 3] {
    screen_cut(whole, ScreenCorner::TopLeft)
}

/// The stream screen's height: most of the strip, with a margin that
/// leaves the strip's own edge showing above and below. The figures
/// inside were drawn for thirty points and scale with this.
const STREAM_SCREEN_H: f32 = 46.0;
/// The stream screen's width: what its figures need at that height.
const STREAM_SCREEN_W: f32 = 284.0 * (STREAM_SCREEN_H / 30.0);

/// Where the stream screen sits: in the vitals strip, just before the
/// register rail that precedes the transport, and centred on the
/// strip's height. Anchored to the transport rather than to the window's
/// centre so it keeps its place on the strip at every width.
fn stream_screen(vitals: egui::Rect, transport: egui::Rect) -> egui::Rect {
    let room = design::px(design::space::ROOM);
    let right = transport.min.x - room * 12.0 - room;
    let left = (right - STREAM_SCREEN_W).max(vitals.min.x + room);
    egui::Rect::from_min_max(
        egui::pos2(left, (vitals.center().y - STREAM_SCREEN_H * 0.5).floor()),
        egui::pos2(right, (vitals.center().y + STREAM_SCREEN_H * 0.5).floor()),
    )
}

/// The gap between the chamfer's nested triangles, and from the cut face
/// to the first. A spacing rung would be too coarse for a mark this
/// small: three rings have to fit inside forty points with room to read.
const CHAMFER_RING_GAP: f32 = 3.0;
/// How many rings the corner carries while the deck sounds.
const CHAMFER_RINGS: usize = 3;

/// The corner's sign of life: triangles nested inside the cut, each set
/// in from the last by one gap, all sharing the cut's own shape. Sound
/// happening is shown at the deck's edge, where nothing else is, so the
/// eye can confirm it without leaving what it was reading.
///
/// A right isosceles triangle inset by `d` keeps its right angle at
/// `(d, d)`. The face moves in by `d` along its normal, to the line
/// `x + y = c − d·√2`; meeting that line at `y = d` puts the far vertex
/// at `x = c − d·√2 − d`, so each leg is `c − d·(2 + √2)`. Rings that
/// would collapse are simply not drawn, so a small window shows fewer
/// rings rather than a scribble.
fn screen_chamfer_rings(whole: egui::Rect) -> Vec<[egui::Pos2; 3]> {
    let [corner, along, _] = screen_chamfer_cut(whole);
    let c = along.x - corner.x;
    (1..=CHAMFER_RINGS)
        .filter_map(|ring| {
            let d = CHAMFER_RING_GAP * ring as f32;
            let leg = c - d * (2.0 + std::f32::consts::SQRT_2);
            (leg > 2.0).then(|| {
                let apex = egui::pos2(corner.x + d, corner.y + d);
                [
                    apex,
                    egui::pos2(apex.x + leg, apex.y),
                    egui::pos2(apex.x, apex.y + leg),
                ]
            })
        })
        .collect()
}

/// The two returns' cables. A send is followed by eye, so each return
/// owns a colour: TAPE warm like the oxide, SHADOW cold like a plate.
const RETURN_INK: [egui::Color32; 2] = [
    egui::Color32::from_rgb(230, 170, 96),
    egui::Color32::from_rgb(130, 176, 255),
];

/// A cable's ink at `amount` of its full strength.
fn tint(ink: egui::Color32, amount: f32) -> egui::Color32 {
    ink.gamma_multiply(amount.clamp(0.0, 1.0))
}

/// Where every zone sits, as a pure function of the window.
///
/// It takes NO state — not focus, not whether the browser is summoned,
/// not what is being shown. That is the whole point: every zone is a
/// fixed place the eye can learn once and read for free forever, and a
/// place that moves has to be found again every time. Zones go quiet, and
/// they go empty, but they never move and they never resize.
///
/// The browser OVERLAYS the field rather than dividing it. Both own the
/// same corner of the screen and neither yields any of it: the field is
/// laid out as though the browser did not exist, and the browser is drawn
/// over the top of it when summoned. Content beneath is hidden for a
/// moment, and hidden is not the same as moved — nothing has to be found
/// again when the browser goes away.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    vitals: egui::Rect,
    breadcrumb: egui::Rect,
    transport: egui::Rect,
    message: egui::Rect,
    browser: egui::Rect,
    /// The whole middle band: session and clip tray together. The
    /// codebook takes all of it.
    field: egui::Rect,
    /// The session: heads and the scene lattice.
    session: egui::Rect,
    /// The clip tray: the sequencer, or quiet.
    clip: egui::Rect,
}

impl Layout {
    fn of(whole: egui::Rect) -> Self {
        let vitals = egui::Rect::from_min_max(
            whole.min,
            egui::pos2(whole.max.x, whole.min.y + PERIPHERY_H),
        );
        // The time end is a fixed number of the design alphabet's largest
        // spacing cells. It never grows with the readout or meter, so a
        // changing song fact cannot move either half of the strip.
        let transport_w = design::px(design::space::VAST) * 8.0;
        let transport_x = (vitals.max.x - transport_w).max(vitals.min.x);
        let breadcrumb =
            egui::Rect::from_min_max(vitals.min, egui::pos2(transport_x, vitals.max.y));
        let transport = egui::Rect::from_min_max(egui::pos2(transport_x, vitals.min.y), vitals.max);
        let message = egui::Rect::from_min_max(
            egui::pos2(whole.min.x, whole.max.y - PERIPHERY_H),
            whole.max,
        );
        // The browser holds the left of the middle band, INSIDE the strips
        // rather than beside them: vitals and messages speak for the whole
        // app, while the browser is content and sits with the content.
        //
        // The band stands off the window's sides by the frame rail, so
        // the shell material runs unbroken from the vitals, down both
        // sides, into the message strip.
        let band = egui::Rect::from_min_max(
            egui::pos2(whole.min.x + FRAME_W, vitals.max.y),
            egui::pos2(whole.max.x - FRAME_W, message.min.y),
        );
        let browser =
            egui::Rect::from_min_max(band.min, egui::pos2(band.min.x + BROWSER_W, band.max.y));
        // The clip tray is cut off the FOOT of the field, fixed: the
        // session above keeps the same shape whether or not the tray has
        // anything to show.
        let clip_top = (band.max.y - CLIP_H).max(band.min.y);
        let session = egui::Rect::from_min_max(band.min, egui::pos2(band.max.x, clip_top));
        let clip = egui::Rect::from_min_max(egui::pos2(band.min.x, clip_top), band.max);
        Self {
            vitals,
            breadcrumb,
            transport,
            message,
            browser,
            field: band,
            session,
            clip,
        }
    }
}

/// The centre of one shown track's board bus.
///
/// Kept pure so every session layer asks the same geometry rather than
/// independently approximating where a column carries current.
fn bus_x(field: egui::Rect, slot: usize) -> f32 {
    Stage::head_rect(field, slot).center().x
}

impl Stage {
    /// A resting plane: where a thing is, carrying structure and not news.
    pub(super) fn square(&self) -> egui::Color32 {
        self.alphabet().surface.color
    }

    /// The marked one. Exactly one thing on the screen is ever this.
    pub(super) fn focused(&self) -> egui::Color32 {
        self.alphabet().focus.color
    }

    /// What a refusal is drawn in.
    fn refusal_ink(&self) -> egui::Color32 {
        self.alphabet().ink.color
    }

    /// Where focus WILL be when it comes back, drawn while it is
    /// somewhere else — a whole rung below focus, so the rule holds that
    /// only one thing is focus-bright.
    pub(super) fn resting(&self) -> egui::Color32 {
        self.alphabet().ink.color
    }

    /// What the clip tray is drawn through while the cursor is not in it.
    ///
    /// The veil takes the surface TOWARD THE GROUND, so which way it
    /// pulls turns over with the polarity: pulling a light page toward
    /// black would make the resting tray louder than the live one, which
    /// is the opposite of what a veil is for.
    fn veil(&self) -> egui::Color32 {
        match self.polarity {
            design::Polarity::Dark => egui::Color32::from_black_alpha(150),
            design::Polarity::Light => egui::Color32::from_white_alpha(150),
        }
    }

    /// How many parameter rows the band shows at once, and only whole
    /// ones. A pure function of the tray, like every other capacity here.
    fn chain_capacity(tray: egui::Rect) -> usize {
        let margin = design::px(design::space::ROOM);
        let cards = chain::rows_that_fit(tray.height() - margin - CHAIN_HEAD_H, CHAIN_PITCH);
        let pieces = chain::rows_that_fit(
            tray.height()
                - margin
                - (strip::HEAD_H + 4.0 + 3.0 + strip::FIGURE_MAX_H + 4.0)
                - strip::FOOT_H
                - 6.0,
            strip::ROW_H,
        );
        cards.min(pieces)
    }

    /// Draw one frame: read the keyboard, advance time, paint the stage.
    /// Where focus is standing, which is what every key is conditioned on.
    /// Draw the palette and run whatever was chosen.
    ///
    /// The list is the CURRENT scope's vocabulary — the same rows the
    /// codebook would show — so the palette answers "what can I press
    /// right now" rather than listing verbs that would be refused if
    /// picked.
    fn pump_palette(&mut self, ctx: &egui::Context) -> Option<StageIntent> {
        if !self.palette.is_open() {
            return None;
        }
        let scope = self.scope_context();
        let entries: Vec<&keymap::Entry> = keymap::palette_entries()
            .iter()
            .filter(|entry| entry.scope == scope)
            .collect();
        let commands: Vec<crate::ui::palette::Command> =
            entries.iter().map(|entry| entry.command).collect();
        // The palette is stock chrome, so it reads the runtime theme
        // rather than the alphabet — and it is handed the one that
        // matches the ground the stage is on, or it would be a dark
        // window over a paper page.
        let theme = match self.polarity {
            design::Polarity::Dark => crate::ui::theme::Theme::dark(),
            design::Polarity::Light => crate::ui::theme::Theme::light(),
        };
        let choice = self.palette.show(ctx, &theme, &commands, &[])?;
        let crate::ui::palette::Choice::Command(id) = choice else {
            // No long-form commands are offered here yet, so a typed line
            // has nothing to mean. Saying nothing is the honest answer.
            return None;
        };
        entries
            .iter()
            .find(|entry| entry.command.id == id)
            .map(|entry| entry.intent)
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        crate::ui::nav_cursor::configure(
            ui.ctx(),
            self.utility.prefs().reduced_motion,
            self.utility.prefs().cursor_energy,
        );
        crate::ui::nav_cursor::begin_frame(ui.ctx());
        self.begin_frame();
        if self.poll_library() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        // A utility room is a true modal: it gets the frame's keyboard
        // before the musical surface and may close itself with Escape.
        self.update_utility(ui.ctx());
        let utility_open = self.utility.is_open();

        // `:` summons the palette, the same key it answers to in the frame
        // before this one. Checked before anything else reads the
        // keyboard, and not while it is already open, so a held key
        // cannot reset what has been typed into it.
        if !utility_open
            && !self.palette.is_open()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Colon))
        {
            self.palette.open();
        }
        if !utility_open && let Some(intent) = self.pump_palette(ui.ctx()) {
            let _ = self.apply(intent);
        }
        let utility_open = self.utility.is_open();
        // While the palette is open it owns the keyboard OUTRIGHT. It
        // consumes the keys it uses itself; this is about the rest —
        // typing "mute" to find a verb must not also play the transport
        // on its way past.
        let palette_open = self.palette.is_open() || utility_open;

        self.hold_browser_for_exit();

        let collect_text = self.collects_text();
        let grammar_owns_escape = self.grammar_owns_escape();
        let scope = self.scope_context();
        let selection_scope = self.selection_scope();
        let selection_held = selection_scope && ui.input(|input| input.key_down(egui::Key::X));
        let selection_pressed = selection_scope
            && ui.input(|input| {
                input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::X,
                            pressed: true,
                            repeat: false,
                            modifiers,
                            ..
                        } if *modifiers == egui::Modifiers::NONE
                    )
                })
            });
        let inputs = if palette_open {
            Vec::new()
        } else {
            ui.input_mut(|input| {
                let chords = input::consume_chords(input, scope, |modifiers, key| {
                    grammar_owns_escape && modifiers == Mods::NONE && key == Key::Escape
                });
                let pressed = |wanted: Key| chords.iter().any(|(_, key)| *key == wanted);
                let questionmark_consumed = pressed(Key::Questionmark);
                let space_consumed = pressed(Key::Space);
                let mut stage_inputs: Vec<keymap::StageInput> = chords
                    .iter()
                    .map(|(modifiers, key)| keymap::StageInput::Chord(*modifiers, *key))
                    .collect();
                if collect_text {
                    for event in &input.events {
                        let egui::Event::Text(text) = event else {
                            continue;
                        };
                        // A physical '?' is the codebook chord. egui also emits
                        // it as text; admitting both would make one keystroke do
                        // two things. Pasted '?' remains ordinary filter text.
                        if questionmark_consumed && text == "?" {
                            continue;
                        }
                        // Space is a global transport chord even while the
                        // browser owns text. As with '?', admit the physical
                        // key exactly once while leaving pasted whitespace
                        // inside longer text untouched.
                        if space_consumed && text == " " {
                            continue;
                        }
                        stage_inputs.extend(text.chars().map(keymap::StageInput::Text));
                    }
                }
                stage_inputs
            })
        };
        self.take_inputs(inputs, selection_held, selection_pressed);

        // Inside a clip, the letters may be pitches. Read after the
        // stage's own chords, so `^T` is never read as a T.
        let update = self
            .pitch_entry_mode()
            .map(|mode| self.midi_typing.update(ui.ctx(), mode));
        let enter_held = ui.input(|input| input.key_down(egui::Key::Enter));
        self.take_pitch_entry(update, enter_held);

        // How much the window can show decides how the view follows the
        // cursor; the following itself is the core's.
        let layout = Layout::of(ui.available_rect_before_wrap());
        self.follow_cursor(
            Self::strip_capacity(layout.session),
            Self::scene_capacity(layout.session),
            Self::chain_capacity(layout.clip),
        );

        // The clock. The engine's own position when the host read one
        // back this frame — so the playhead is what SOUNDED — and the
        // same fallback the legacy control plane uses when it did not:
        // the UI's stable frame delta, with continuous frames only while
        // time is actually passing.
        let dt = ui.ctx().input(|input| input.stable_dt);
        self.tick_clock(dt);
        if self.wants_repaint() {
            ui.ctx().request_repaint();
        }

        self.draw(ui);
    }

    /// How many track columns the field can show. The strip's geometry is
    /// fixed by rule, so this is a pure function of the window and never of
    /// where the cursor stands.
    fn strip_capacity(field: egui::Rect) -> usize {
        let gap = column_gap();
        let margin = design::px(design::space::ROOM);
        // The master's column and the gap before it are not the strip's
        // to spend: a track drawn under the master would be a track the
        // performer cannot see.
        let usable = field.width() - margin * 2.0 - ADDRESS_W + gap - (TRACK_W + gap);
        if usable <= 0.0 {
            return 1;
        }
        ((usable / (TRACK_W + gap)).floor() as usize).max(1)
    }

    /// How many scene rows fit under the heads, and only whole ones.
    fn scene_capacity(field: egui::Rect) -> usize {
        let gap = row_gap();
        let margin = design::px(design::space::ROOM);
        let head_bottom = Self::head_rect(field, 0).max.y;
        scenes::rows_that_fit(field.max.y - margin - (head_bottom + section_gap()), gap)
    }

    /// The scenes the lattice currently shows, as a range into the
    /// session's rows. The vertical twin of [`Self::strip_window`].
    fn scene_window(&self, field: egui::Rect) -> std::ops::Range<usize> {
        let count = self.song.session.scenes.len();
        if count == 0 {
            return 0..0;
        }
        let first = self.scene_offset.min(count - 1);
        let last = first.saturating_add(Self::scene_capacity(field)).min(count);
        first..last
    }

    /// The session as drawn: the root lattice, and the shade its cursor
    /// takes. The session is on screen whenever the cursor is on it OR
    /// inside a clip beneath it — the tray below is where focus went, and
    /// the session's cursor is then RESTING: where focus will land when
    /// it comes back, one rung down from where it is. Inside a TRACK
    /// (the calibration field) the session is not drawn at all.
    fn session_lattice(&self) -> Option<(&FocusLattice, egui::Color32)> {
        let FocusScope::Lattice(lattice) = self.focus.levels().first()? else {
            return None;
        };
        // Exactly one thing on the screen is focus-bright. While the
        // band or the browser holds the keys, the session's cursor rests
        // — it says where focus will land when it comes back, not where
        // it is.
        let holds_the_keys = self.chain.is_none() && self.browser.is_none();
        match (self.focus.depth(), self.inside) {
            (1, _) if holds_the_keys => Some((lattice, self.focused())),
            (1, _) => Some((lattice, self.resting())),
            (_, Some(_)) => Some((lattice, self.resting())),
            _ => None,
        }
    }

    /// The tracks the strip currently shows, as a range into the song's
    /// order. `show` has already moved the window to contain the cursor,
    /// so drawing never decides where to look — it only draws what was
    /// decided. Everything that lines up under a head asks here, so the
    /// strip and the lattice can never disagree about which tracks are on
    /// screen.
    fn strip_window(&self, field: egui::Rect) -> std::ops::Range<usize> {
        let count = self.song.tracks.len();
        if count == 0 {
            return 0..0;
        }
        let first = self.strip_offset.min(count - 1);
        let last = first.saturating_add(Self::strip_capacity(field)).min(count);
        first..last
    }

    /// The master's column, pinned to the right edge of the field.
    ///
    /// It does NOT come from `head_rect`: every other column's place is a
    /// function of how far along the strip it is, and the master's whole
    /// point is that it is not along the strip at all. It belongs to the
    /// song rather than to anything in it, so it stays where the eye can
    /// always find it however many tracks scroll past.
    fn master_rect(field: egui::Rect) -> egui::Rect {
        let margin = design::px(design::space::ROOM);
        egui::Rect::from_min_size(
            egui::pos2(field.max.x - margin - TRACK_W, field.min.y + margin),
            egui::vec2(TRACK_W, TRACK_H),
        )
    }

    fn head_rect(field: egui::Rect, slot: usize) -> egui::Rect {
        let gap = column_gap();
        let margin = design::px(design::space::ROOM);
        egui::Rect::from_min_size(
            egui::pos2(
                field.min.x + margin + ADDRESS_W + slot as f32 * (TRACK_W + gap),
                field.min.y + margin,
            ),
            egui::vec2(TRACK_W, TRACK_H),
        )
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let whole = ui.available_rect_before_wrap();
        let painter = ui.painter().clone();
        let phase = Phase::of(
            self.transport.motion().is_rolling(),
            self.transport.beat_phase(),
        );

        // The constitution: a thin fixed periphery around one sovereign
        // field. The strips hold display only; nothing in them is ever
        // focusable, and their geometry never changes.
        let Layout {
            vitals,
            breadcrumb,
            transport,
            message,
            browser,
            field,
            session,
            clip,
        } = Layout::of(whole);

        // Separation is a STEP IN VALUE, not a rule drawn between things.
        // Two planes that differ in lightness are already divided; a line
        // laid along the seam restates a boundary the eye has read, and a
        // screen full of restatements is the noise floor this surface
        // spends its budget keeping down.
        //
        // The consequence is worth the trade: a stroke now always MEANS
        // something. Every line left on this surface is a sign — a gate, a
        // refusal, the signature — and none of them is furniture.
        // The frame paints its OWN ground first.
        //
        // It used to rely on the host's clear colour being the same one,
        // which held only as long as the two agreed — and the moment the
        // ground became switchable that agreement was a thing to get
        // wrong rather than a constant. A surface that is only the right
        // colour when somebody else guessed correctly is not a surface
        // that owns its own appearance.
        //
        // The casing is painted first and the field is cut out of it: the
        // whole window is shell material, and the field is the one
        // place the ground shows through. Vitals, message strip and the
        // two side rails are therefore one continuous frame, not four
        // pieces that happen to touch.
        painter.rect_filled(whole, 0.0, shell_base(self.polarity));
        // The field is a physical cut into that metal plane. One opaque,
        // offset silhouette supplies depth without blur, gloss or a
        // gradient; the ground laid next covers its upper-left overlap.
        painter.rect_filled(
            field
                .translate(egui::vec2(circuit::SHADOW_X, circuit::SHADOW_Y))
                .intersect(whole),
            0.0,
            circuit::shadow_ink(self.alphabet().ground.color),
        );
        painter.rect_filled(field, 0.0, self.alphabet().ground.color);
        // The ground's own material. Quietest thing on the surface, says
        // nothing, and therefore may cover everything — and what it does
        // say without saying it is that this app is a lattice.
        ornament::lattice(
            &painter,
            field,
            self.alphabet().surface.color,
            design::px(design::space::VAST),
        );
        self.draw_shell_plating(&painter, vitals, transport, message, field, phase);
        // The regions get an edge each.
        //
        // This is a DEPARTURE from the rule above, and worth saying so
        // rather than quietly breaking it: value alone divides two planes
        // well on a black ground, where the resting rungs sit against
        // nothing. On PAPER the same rungs are three steps inside a
        // narrow band near white, and a seam between two of them is a
        // difference the eye has to look for. The line is doing work the
        // value cannot do here, which is the test any mark on this
        // surface has to pass.
        //
        // It is the structure rung and one pixel: enough to bound a
        // region, not enough to become the thing you see first.
        // The browser is NOT bounded here. Its rectangle overlaps the
        // session's, so an edge around it while it is away is a line
        // through the middle of something else — and while it is
        // arriving, an edge drawn at the destination is a line the
        // panel has not reached yet. Its edge travels with it, in the
        // slide below.
        // Brushed rather than ruled. The line still does the value's work
        // on paper; what changes is the hand that drew it. A frame with
        // pressure in it says the deck was MADE, and the same seed every
        // frame says it was made once.
        //
        // Two weights, by rank. The strips are periphery and keep the
        // hairline; the session and the tray are the primary casings —
        // the two windows cut into the deck — and take the heavy line,
        // which is what `Weight::Heavy` is for. Same rung, same ink: the
        // weight says what kind of edge this is, not how loud.
        for (n, (region, weight)) in [
            (vitals, Weight::Hair),
            (message, Weight::Hair),
            (session, Weight::Heavy),
            (clip, Weight::Heavy),
        ]
        .into_iter()
        .enumerate()
        {
            let edge = self.alphabet().edge.color;
            kit::cached(
                &painter,
                egui::Id::new(("stage-frame", n)),
                region,
                (edge, weight),
                |out| {
                    circuit::panel_frame_variant(out, region, weight, edge, n as u8);
                },
            );
        }
        // The screen's own chamfer, fullscreen only. Painted AFTER the
        // strip's frame so the cut goes through the line as well as the
        // material, the way a corner cut off a casing takes its edge with
        // it; then the cut gets the edge back along its new face.
        if ui.ctx().input(|i| i.viewport().fullscreen.unwrap_or(false)) {
            let ground = self.alphabet().ground.color;
            let edge = self.alphabet().edge.color;
            kit::cached(
                &painter,
                egui::Id::new("stage-screen-chamfer"),
                whole,
                (ground, edge),
                |out| {
                    for corner in ScreenCorner::ALL {
                        let [apex, along, down] = screen_cut(whole, corner);
                        out.push(egui::Shape::convex_polygon(
                            vec![apex, along, down],
                            ground,
                            egui::Stroke::NONE,
                        ));
                        circuit::trace(out, &[along, down], Weight::Hair, edge);
                    }
                },
            );
            // While the deck SOUNDS — transport rolling, and an engine
            // behind it to roll — the cut fills with rings. Both are
            // required: a rolling transport with no engine is a clock
            // with nothing to drive, and the corner should not claim
            // sound that is not being made. Uncached, because it
            // breathes with the beat like every other live mark.
            if phase.rolling && self.vitals.running() {
                let alpha = self.alphabet();
                let live = motion::pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
                let mut rings = Vec::new();
                for tri in screen_chamfer_rings(whole) {
                    circuit::trace(
                        &mut rings,
                        &[tri[0], tri[1], tri[2], tri[0]],
                        Weight::Hair,
                        live,
                    );
                }
                painter.extend(rings);
            }
        }
        // The meter register: the song's real beat count and current beat,
        // just before the transport aperture. No fixed barcode masquerades
        // as telemetry here; every pad is one beat in the active meter.
        {
            let edge = self.alphabet().edge.color;
            let active = if phase.rolling {
                self.alphabet().live.color
            } else {
                self.alphabet().ink.color
            };
            let room = design::px(design::space::ROOM);
            let strip = egui::Rect::from_min_max(
                egui::pos2(transport.min.x - room * 12.0, vitals.min.y + room * 0.6),
                egui::pos2(transport.min.x - room, vitals.max.y - room * 0.6),
            );
            if strip.min.x > breadcrumb.min.x + room * 10.0 {
                let place = self.transport.place(&self.song);
                kit::cached(
                    &painter,
                    egui::Id::new("stage-register"),
                    strip,
                    (edge, active, place.beat, place.beats_per_bar),
                    |out| {
                        let y = strip.center().y;
                        circuit::trace(
                            out,
                            &[
                                egui::pos2(strip.min.x + 8.0, y),
                                egui::pos2(strip.max.x - 8.0, y),
                            ],
                            Weight::Hair,
                            edge,
                        );
                        let beats = place.beats_per_bar.clamp(1, 16);
                        let run = strip.width() - 32.0;
                        for beat in 1..=beats {
                            let t = if beats == 1 {
                                0.5
                            } else {
                                (beat - 1) as f32 / (beats - 1) as f32
                            };
                            let at = egui::pos2(strip.left() + 16.0 + run * t, y);
                            let current = beat == place.beat.min(beats);
                            circuit::pad(
                                out,
                                at,
                                if current {
                                    circuit::PAD + 1.0
                                } else {
                                    circuit::PAD - 1.0
                                },
                                if current { active } else { edge },
                                current,
                            );
                        }
                    },
                );
            }
        }
        // The seam between the breadcrumb and the clock: one stroke.
        {
            let edge = self.alphabet().edge.color;
            let x = transport.min.x;
            let seam = egui::Rect::from_min_max(
                egui::pos2(x - 4.0, vitals.min.y),
                egui::pos2(x + 4.0, vitals.max.y),
            );
            kit::cached(
                &painter,
                egui::Id::new("stage-vitals-seam"),
                seam,
                edge,
                |out| {
                    circuit::trace(
                        out,
                        &[
                            egui::pos2(x, vitals.min.y + 6.0),
                            egui::pos2(x, vitals.max.y - 6.0),
                        ],
                        Weight::Hair,
                        edge,
                    );
                },
            );
        }

        self.draw_breadcrumb(&painter, breadcrumb);
        self.draw_stream(&painter, stream_screen(vitals, transport), phase);
        self.draw_engine(&painter, message);
        self.draw_transport(&painter, transport);
        self.draw_shell_register(&painter, message);
        self.draw_message(&painter, message);
        // The codebook takes the whole field while it is up. It is a
        // DISPLAY mode, not a scope: focus never enters it, and the
        // cursor underneath is exactly where it was left.
        // The tray reports where its cursor stands while it draws; a
        // covered tray reports nothing, and a menu with nowhere to point
        // is not drawn.
        let mut trig_anchor = None;
        let screen_state = crate::shell::screen::State::new(
            if phase.rolling { phase.beat } else { 0.0 },
            if phase.rolling {
                0.18 + phase.pulse() * 0.72
            } else {
                0.0
            },
        );
        if self.sample.is_some() {
            self.draw_sample_editor(&painter, field, phase);
            if self.polarity == design::Polarity::Dark {
                crate::shell::screen::register(&painter, field, screen_state);
            }
        } else if self.help {
            self.draw_help(&painter, field);
            if self.polarity == design::Polarity::Dark {
                crate::shell::screen::register(&painter, field, screen_state);
            }
        } else {
            // Stacked: the session above, the clip tray below. The tray
            // shows whatever clip the session cursor is on, and is only
            // FOCUSED once entered — Ableton's session over its clip
            // detail, an Elektron's track keys over its trig keys.
            self.draw_field(&painter, session, phase);
            if self.polarity == design::Polarity::Dark {
                crate::shell::screen::register(&painter, session, screen_state);
            }
            // One detail region, and the band and the sequencer are two
            // things to put in it. The band wins while it is showing:
            // sound design and sequencing are separate spaces, and the
            // one you are in is the one you asked for.
            if self.chain.is_some() {
                self.draw_chain(ui.painter(), clip, phase);
            } else {
                trig_anchor = self.draw_clip(ui, clip);
                if self.polarity == design::Polarity::Dark {
                    crate::shell::screen::register(&painter, clip, screen_state);
                }
            }
        }
        // Last, and over the top of everything in the field: the browser
        // is a window above the work, not a division of it.
        //
        // It SLIDES, from the edge it belongs to. Motion here is not
        // decoration: a panel that simply appears has to be found, while
        // one that arrives from somewhere has already told the eye where
        // it came from and where it will go back to.
        let open = ui.ctx().animate_bool_with_time(
            egui::Id::new("stage-browser-slide"),
            self.browser.is_some(),
            BROWSER_SLIDE_S,
        );
        if open > 0.0 && self.browser_leaving.is_some() {
            let slid = browser.translate(egui::vec2(-browser.width() * (1.0 - open), 0.0));
            // Clipped to the field, so the part that has not arrived is
            // not drawn over the periphery on its way in.
            let painter = painter.with_clip_rect(field);
            self.draw_browser(&painter, slid);
            if self.polarity == design::Polarity::Dark {
                crate::shell::screen::register(&painter, slid, screen_state);
            }
            // Its own edge, at wherever it has got to. Drawn with the
            // panel rather than with the other regions: a border that
            // waited at the destination would announce the arrival
            // before the thing arrived.
            painter.rect_stroke(
                slid,
                0.0,
                egui::Stroke::new(1.0, self.alphabet().edge.color),
                egui::StrokeKind::Inside,
            );
        }
        // Over everything, because it is about one thing: the trig menu
        // is a callout, and a callout drawn under anything is a callout
        // pointing through it.
        self.draw_trig_menu(&painter, whole, trig_anchor);
        self.draw_plock_editor(&painter, whole);
        // Last of all, because the machine room is not part of the
        // musical surface: it stands in front of the whole of it.
        self.draw_utility(ui);
        crate::ui::nav_cursor::paint(ui.ctx());
    }

    /// The fixed shell: two raised faceplates, a recessed transport glass,
    /// an engraved message well, focus-depth cells on the left rail, and
    /// the actual master level on the right. The geometry is ceremonial;
    /// every changing mark is a value already owned by the stage.
    fn draw_shell_plating(
        &self,
        painter: &egui::Painter,
        vitals: egui::Rect,
        transport: egui::Rect,
        message: egui::Rect,
        field: egui::Rect,
        phase: Phase,
    ) {
        let alpha = self.alphabet();
        let base = shell_base(self.polarity);
        let plate = shell_plate(self.polarity);
        let edge = alpha.edge.color;
        let mut shell = Vec::new();

        // A thin raised insert runs down each side. The true-black base is
        // left visible around it, while the focus and level instruments sit
        // on metal rather than floating in the void.
        for rail in [
            egui::Rect::from_min_max(
                egui::pos2(vitals.left() + 3.0, field.top() + 3.0),
                egui::pos2(field.left() - 2.0, field.bottom() - 3.0),
            ),
            egui::Rect::from_min_max(
                egui::pos2(field.right() + 2.0, field.top() + 3.0),
                egui::pos2(vitals.right() - 3.0, field.bottom() - 3.0),
            ),
        ] {
            shell.push(egui::Shape::rect_filled(rail, 0.0, plate));
        }

        for (rect, variant) in [
            (vitals.shrink2(egui::vec2(2.0, 3.0)), 2),
            (message.shrink2(egui::vec2(2.0, 3.0)), 3),
        ] {
            circuit::panel_variant(
                &mut shell,
                rect,
                Some(plate),
                base,
                Some((Weight::Heavy, edge)),
                variant,
            );
            circuit::panel_frame_variant(
                &mut shell,
                rect.shrink(3.0),
                Weight::Hair,
                edge.gamma_multiply(0.48),
                variant.wrapping_add(1),
            );
        }

        // Time is a display, not text printed on the chassis.
        let clock_glass = transport.shrink2(egui::vec2(5.0, 8.0));
        circuit::panel_variant(
            &mut shell,
            clock_glass,
            Some(alpha.ground.color),
            plate,
            Some((Weight::Heavy, edge)),
            1,
        );
        circuit::panel_frame_variant(
            &mut shell,
            clock_glass.shrink(3.0),
            Weight::Hair,
            edge.gamma_multiply(0.55),
            3,
        );

        // Messages are engraved into a long dark trough in the lower plate.
        let message_well = message.shrink2(egui::vec2(5.0, 9.0));
        circuit::panel_variant(
            &mut shell,
            message_well,
            Some(alpha.ground.color),
            plate,
            Some((Weight::Hair, edge)),
            0,
        );
        circuit::panel_frame_variant(
            &mut shell,
            message_well.shrink(3.0),
            Weight::Hair,
            edge.gamma_multiply(0.42),
            2,
        );

        // The rails themselves are structural seams with hard angular
        // shoulders. Their changing contents are added below.
        for x in [field.left() - FRAME_W * 0.5, field.right() + FRAME_W * 0.5] {
            let top = field.top() + 8.0;
            let bottom = field.bottom() - 8.0;
            circuit::trace(
                &mut shell,
                &[
                    egui::pos2(x, top),
                    egui::pos2(x, top + 18.0),
                    egui::pos2(x + 3.0, top + 21.0),
                    egui::pos2(x + 3.0, bottom - 21.0),
                    egui::pos2(x, bottom - 18.0),
                    egui::pos2(x, bottom),
                ],
                Weight::Hair,
                edge,
            );
        }

        // Left rail: one real cell per focus level, up to the stack's cap.
        let depth = self.focus.depth().min(MAX_DEPTH);
        let left_x = field.left() - FRAME_W * 0.5;
        for level in 0..MAX_DEPTH {
            circuit::pad(
                &mut shell,
                egui::pos2(left_x, field.top() + 42.0 + level as f32 * 12.0),
                circuit::PAD,
                if level < depth { alpha.ink.color } else { edge },
                level < depth,
            );
        }

        // Right rail: the real master peak, repeated at the casing edge so
        // level remains visible while any field or overlay owns the centre.
        let master = self.meters.master().level.peak().clamp(0.0, 1.0);
        let meter = egui::Rect::from_min_max(
            egui::pos2(field.right() + 3.0, field.top() + 28.0),
            egui::pos2(field.right() + 9.0, field.bottom() - 28.0),
        );
        circuit::tick_bar(
            &mut shell,
            meter,
            32,
            master,
            if phase.rolling {
                alpha.live.color
            } else {
                alpha.ink.color
            },
            edge.gamma_multiply(0.45),
            false,
        );
        painter.extend(shell);
    }

    /// The bottom-left register is state, not a maker's mark: engine life,
    /// track count, scene count and focus depth. The binary cells keep the
    /// inherited-machine character while their labels make the facts honest.
    fn draw_shell_register(&self, painter: &egui::Painter, message: egui::Rect) {
        let alpha = self.alphabet();
        let y = message.center().y;
        let engine_ink = if self.vitals.running() {
            alpha.live.color
        } else {
            alpha.edge.color
        };
        let mut marks = Vec::new();
        Sign::Engine.paint(
            &mut marks,
            egui::Rect::from_center_size(
                egui::pos2(message.left() + 22.0, y),
                egui::Vec2::splat(18.0),
            ),
            Weight::Hair,
            engine_ink,
        );
        let facts = [
            ('T', self.song.tracks.len().min(255) as u32),
            ('S', self.song.session.scenes.len().min(255) as u32),
            ('D', self.focus.depth().min(255) as u32),
        ];
        let font = egui::FontId::monospace(9.0);
        for (index, (label, value)) in facts.into_iter().enumerate() {
            let x = message.left() + 48.0 + index as f32 * 44.0;
            painter.text(
                egui::pos2(x, y),
                egui::Align2::LEFT_CENTER,
                label,
                font.clone(),
                alpha.edge.color,
            );
            circuit::binary(
                &mut marks,
                egui::pos2(x + 10.0, y - 1.5),
                2.5,
                value,
                8,
                alpha.ink.color,
            );
        }
        painter.extend(marks);
    }

    /// The trig menu: a chamfered casing over the sequencer with a wedge
    /// of a tail landing on the trig under the cursor. The casing is the
    /// deck's material with the deck's heavy edge; the tail is cut from
    /// the same piece, so casing and tail read as one shape and not as a
    /// box with an arrow beside it. The head names the trig; on a
    /// slicing track a strip shows the file with its cuts and the one
    /// this trig plays; the list is the voice's parameters as sliders,
    /// each showing the knob and, when the trig holds one, the lock; the
    /// TRIG row leads to the trig's own verbs. The cursor row wears the
    /// cursor's brackets.
    fn draw_trig_menu(
        &self,
        painter: &egui::Painter,
        whole: egui::Rect,
        anchor: Option<egui::Rect>,
    ) {
        let Some(menu) = self.trig_menu else {
            return;
        };
        let Some(anchor) = anchor else {
            return;
        };
        let Some((_, trig)) = self.trig_under_cursor() else {
            return;
        };
        let alpha = self.alphabet();
        let rows = self
            .menu_rows_under_cursor()
            .map(|(_, _, rows)| rows)
            .unwrap_or_else(|| vec![MenuRow::Trig]);
        let shown_track = self
            .clip_in_view()
            .and_then(|shown| self.song.tracks.get(shown.track));
        let voice = shown_track.map_or("VOICE", |track| trig_menu::voice_of(track).0.name);
        let slice_row = rows.iter().find_map(|row| match row {
            MenuRow::Slice(slice) => Some(*slice),
            _ => None,
        });
        let strip_h = if menu.page == Page::Locks && slice_row.is_some() {
            trig_menu::STRIP_H
        } else {
            0.0
        };
        let total = self.trig_menu_rows(menu);
        let visible = total.min(trig_menu::MAX_ROWS);
        let bubble = trig_menu::place(anchor, whole, visible, strip_h);
        let panel = bubble.panel;
        let inner = panel.shrink2(egui::vec2(16.0, trig_menu::MARGIN));
        let [tail_l, tail_r, apex] = bubble.tail;
        let reach = panel.union(egui::Rect::from_points(&bubble.tail));
        kit::cached(
            painter,
            egui::Id::new("stage-trig-menu-shell"),
            reach,
            (
                alpha.surface.color,
                alpha.ground.color,
                alpha.ink.color,
                (apex.x * 2.0) as i32,
                (apex.y * 2.0) as i32,
                bubble.above,
            ),
            |out| {
                circuit::panel_variant(
                    out,
                    panel,
                    Some(alpha.surface.color),
                    alpha.ground.color,
                    Some((Weight::Heavy, alpha.ink.color)),
                    1,
                );
                let into = if bubble.above { -3.0 } else { 3.0 };
                out.push(egui::Shape::convex_polygon(
                    vec![
                        egui::pos2(tail_l.x, tail_l.y + into),
                        egui::pos2(tail_r.x, tail_r.y + into),
                        apex,
                    ],
                    alpha.surface.color,
                    egui::Stroke::NONE,
                ));
                circuit::trace(out, &[tail_l, apex], Weight::Heavy, alpha.ink.color);
                circuit::trace(out, &[tail_r, apex], Weight::Heavy, alpha.ink.color);
                circuit::pad(out, apex, circuit::PAD, alpha.ink.color, true);
                circuit::panel_frame_variant(
                    out,
                    panel.shrink(5.0),
                    Weight::Hair,
                    alpha.edge.color,
                    3,
                );
                let sign = egui::Rect::from_center_size(
                    egui::pos2(inner.left() + 9.0, inner.top() + 9.0),
                    egui::Vec2::splat(16.0),
                );
                Sign::General((trig.start_ticks / PATTERN_STEP_TICKS % 32) as u8).paint(
                    out,
                    sign,
                    Weight::Hair,
                    alpha.edge.color,
                );
                let rail_y = inner.top() + trig_menu::HEAD_H - 8.0;
                circuit::rail(
                    out,
                    egui::pos2(inner.left(), rail_y),
                    egui::pos2(inner.right(), rail_y),
                    &[0.0, 0.8, 1.0],
                    alpha.edge.color,
                );
            },
        );
        let step = trig.start_ticks / PATTERN_STEP_TICKS;
        block::paint(
            painter,
            egui::Id::new("stage-trig-menu-title"),
            egui::pos2(inner.left() + 24.0, inner.top()),
            egui::Align2::LEFT_TOP,
            block::unit::TITLE,
            &format!("TRIG {:02}", step + 1),
            alpha.ink.color,
        );
        let held = rows
            .iter()
            .filter(|row| match row {
                MenuRow::Trig => false,
                MenuRow::Slice(slice) => slice.lock.is_some(),
                MenuRow::Param(row) => row.lock.is_some(),
            })
            .count();
        let facts = match menu.page {
            Page::Locks => format!("{}  ·  {held} LOCKED", voice.to_uppercase()),
            Page::Trig => format!(
                "{}  VEL {:>3}  LEN {}  {}%",
                sequencer::sequence_grid::note_name(trig.midi),
                trig.velocity,
                sequencer::grid_resolution::length_label(trig.length_ticks),
                (trig.probability * 100.0).round() as u32,
            ),
        };
        painter.text(
            egui::pos2(inner.left(), inner.top() + 27.0),
            egui::Align2::LEFT_TOP,
            facts,
            egui::FontId::monospace(11.0),
            alpha.ink.color,
        );

        // The slice strip: the file, its cuts, and the cut this trig
        // plays washed in the focus ink. The lock's cut when there is a
        // lock, the knob's when not, so the strip always shows what
        // will sound.
        if let Some(slice) = slice_row
            && strip_h > 0.0
        {
            let strip = egui::Rect::from_min_size(
                egui::pos2(inner.left(), inner.top() + trig_menu::HEAD_H),
                egui::vec2(inner.width(), strip_h - 6.0),
            );
            let device = shown_track.and_then(|track| track.chain.first());
            self.draw_slice_strip(painter, strip, slice, device);
        }

        let list_top = inner.top() + trig_menu::HEAD_H + strip_h;
        for shown in 0..visible {
            let index = menu.offset + shown;
            let y = list_top + shown as f32 * trig_menu::ROW_H;
            let row = egui::Rect::from_min_max(
                egui::pos2(inner.left(), y + 1.0),
                egui::pos2(inner.right(), y + trig_menu::ROW_H - 1.0),
            );
            let on = index == menu.row;
            if on {
                crate::ui::nav_cursor::claim(
                    painter,
                    ("stage-trig-menu-cursor", index),
                    row,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Overlay,
                    alpha.focus.color,
                );
            }
            let ink = if on {
                alpha.focus.color
            } else {
                alpha.ink.color
            };
            match menu.page {
                Page::Trig => {
                    painter.text(
                        egui::pos2(row.left() + 14.0, row.center().y),
                        egui::Align2::LEFT_CENTER,
                        TrigAction::ALL[index].label(&trig),
                        egui::FontId::monospace(13.0),
                        ink,
                    );
                }
                Page::Locks => match rows.get(index) {
                    Some(MenuRow::Trig) | None => {
                        painter.text(
                            egui::pos2(row.left() + 14.0, row.center().y),
                            egui::Align2::LEFT_CENTER,
                            "TRIG",
                            egui::FontId::monospace(13.0),
                            ink,
                        );
                        painter.text(
                            egui::pos2(row.right() - 6.0, row.center().y),
                            egui::Align2::RIGHT_CENTER,
                            ">",
                            egui::FontId::monospace(13.0),
                            alpha.edge.color,
                        );
                    }
                    Some(MenuRow::Slice(slice)) => {
                        painter.text(
                            egui::pos2(row.left() + 14.0, row.center().y),
                            egui::Align2::LEFT_CENTER,
                            "SLICE",
                            egui::FontId::monospace(12.0),
                            ink,
                        );
                        let (word, word_ink) = match slice.lock {
                            Some(lock) => {
                                (format!("{lock:02} / {:02}", slice.count), alpha.live.color)
                            }
                            None => (
                                format!("{:02} / {:02}", slice.knob, slice.count),
                                alpha.edge.color,
                            ),
                        };
                        painter.text(
                            egui::pos2(row.right() - 4.0, row.center().y),
                            egui::Align2::RIGHT_CENTER,
                            word,
                            egui::FontId::monospace(12.0),
                            word_ink,
                        );
                        painter.text(
                            egui::pos2(row.left() + 128.0, row.center().y),
                            egui::Align2::LEFT_CENTER,
                            "<  LEFT / RIGHT  >",
                            egui::FontId::monospace(10.0),
                            alpha.edge.color,
                        );
                    }
                    Some(MenuRow::Param(lock_row)) => {
                        self.draw_lock_row(painter, row, lock_row, on);
                    }
                },
            }
        }
        if total > visible {
            let hint = |y: f32, glyph: &str| {
                painter.text(
                    egui::pos2(inner.right() - 6.0, y),
                    egui::Align2::RIGHT_CENTER,
                    glyph,
                    egui::FontId::monospace(11.0),
                    alpha.edge.color,
                );
            };
            if menu.offset > 0 {
                hint(list_top - 4.0, "^");
            }
            if menu.offset + visible < total {
                hint(list_top + visible as f32 * trig_menu::ROW_H + 2.0, "v");
            }
        }
    }

    fn draw_plock_editor(&self, painter: &egui::Painter, whole: egui::Rect) {
        let Some(editor) = self.plock_editor.as_ref() else {
            return;
        };
        let alpha = self.alphabet();
        let size = egui::vec2(
            (whole.width() - 48.0).min(820.0),
            (whole.height() - 48.0).min(520.0),
        );
        let panel = egui::Rect::from_center_size(whole.center(), size);
        let inner = panel.shrink(16.0);
        let head_h = 48.0;
        let controls_h = 52.0;
        let list_w = 190.0;
        let body = egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.top() + head_h),
            egui::pos2(inner.right(), inner.bottom() - controls_h),
        );
        let list =
            egui::Rect::from_min_max(body.min, egui::pos2(body.left() + list_w, body.bottom()));
        let graphs = egui::Rect::from_min_max(
            egui::pos2(list.right() + 12.0, body.top()),
            body.right_bottom(),
        );
        let controls = egui::Rect::from_min_max(
            egui::pos2(inner.left(), body.bottom() + 8.0),
            inner.right_bottom(),
        );
        let mut shell = Vec::new();
        circuit::panel_variant(
            &mut shell,
            panel,
            Some(alpha.surface.color),
            alpha.ground.color,
            Some((Weight::Heavy, alpha.ink.color)),
            2,
        );
        circuit::panel_frame_variant(
            &mut shell,
            panel.shrink(5.0),
            Weight::Hair,
            alpha.edge.color,
            4,
        );
        painter.extend(shell);
        block::paint(
            painter,
            egui::Id::new("stage-plock-title"),
            inner.left_top(),
            egui::Align2::LEFT_TOP,
            block::unit::TITLE,
            "PARAMETER LOCKS",
            alpha.ink.color,
        );
        painter.text(
            egui::pos2(inner.right(), inner.top() + 4.0),
            egui::Align2::RIGHT_TOP,
            format!(
                "{} CELLS  ·  {} PARAMS",
                editor.ticks.len(),
                editor.selected_params.len()
            ),
            egui::FontId::monospace(11.0),
            alpha.edge.color,
        );

        let row_h = 20.0;
        let visible = (list.height() / row_h).floor().max(1.0) as usize;
        let offset = editor
            .param_cursor
            .saturating_sub(visible.saturating_sub(1));
        for (shown, param) in editor.params.iter().skip(offset).take(visible).enumerate() {
            let index = offset + shown;
            let row = egui::Rect::from_min_size(
                egui::pos2(list.left(), list.top() + shown as f32 * row_h),
                egui::vec2(list.width(), row_h),
            );
            let selected = editor.selected_params.contains(&index);
            let cursor =
                editor.focus == plock_editor::Focus::Parameters && editor.param_cursor == index;
            if selected {
                painter.rect_filled(row.shrink(1.0), 0.0, alpha.focus.color.gamma_multiply(0.22));
            }
            if cursor {
                let mut marks = Vec::new();
                circuit::brackets(&mut marks, row, 5.0, Weight::Bold, alpha.focus.color);
                painter.extend(marks);
            }
            painter.text(
                row.left_center() + egui::vec2(12.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &param.name,
                egui::FontId::monospace(11.0),
                if selected {
                    alpha.ink.color
                } else {
                    alpha.edge.color
                },
            );
            painter.text(
                row.right_center() - egui::vec2(4.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                if selected { "X" } else { "·" },
                egui::FontId::monospace(11.0),
                if selected {
                    alpha.live.color
                } else {
                    alpha.edge.color
                },
            );
        }

        let selected = editor.selected_param_indices();
        let lanes = selected.len().max(1);
        let lane_h = (graphs.height() / lanes as f32).max(24.0);
        for (lane_index, &param_index) in selected.iter().enumerate() {
            let lane = egui::Rect::from_min_max(
                egui::pos2(graphs.left(), graphs.top() + lane_index as f32 * lane_h),
                egui::pos2(
                    graphs.right(),
                    (graphs.top() + (lane_index + 1) as f32 * lane_h).min(graphs.bottom()),
                ),
            );
            painter.rect_stroke(
                lane.shrink(1.0),
                0.0,
                egui::Stroke::new(1.0, alpha.edge.color.gamma_multiply(0.7)),
                egui::StrokeKind::Inside,
            );
            let param = &editor.params[param_index];
            let bar_w = lane.width() / editor.ticks.len().max(1) as f32;
            for cell in 0..editor.ticks.len() {
                let x0 = lane.left() + cell as f32 * bar_w + 1.0;
                let x1 = lane.left() + (cell + 1) as f32 * bar_w - 1.0;
                let fraction = param.fraction(editor.displayed(param_index, cell));
                let rect = egui::Rect::from_min_max(
                    egui::pos2(x0, lane.bottom() - 3.0 - (lane.height() - 8.0) * fraction),
                    egui::pos2(x1.max(x0 + 1.0), lane.bottom() - 3.0),
                );
                let active = editor.active[cell];
                painter.rect_filled(
                    rect,
                    0.0,
                    if active {
                        alpha.live_dim.color
                    } else {
                        alpha.edge.color.gamma_multiply(0.28)
                    },
                );
                if editor.focus == plock_editor::Focus::Graphs
                    && editor.graph_lane == lane_index
                    && editor.graph_cell == cell
                {
                    let mut marks = Vec::new();
                    circuit::brackets(
                        &mut marks,
                        egui::Rect::from_min_max(
                            egui::pos2(x0, lane.top() + 2.0),
                            egui::pos2(x1.max(x0 + 1.0), lane.bottom() - 2.0),
                        ),
                        4.0,
                        Weight::Bold,
                        alpha.focus.color,
                    );
                    painter.extend(marks);
                }
            }
        }

        let control_on = editor.focus == plock_editor::Focus::Controls;
        if control_on {
            let mut marks = Vec::new();
            circuit::brackets(&mut marks, controls, 7.0, Weight::Bold, alpha.focus.color);
            painter.extend(marks);
        }
        painter.text(
            controls.left_center() + egui::vec2(12.0, -8.0),
            egui::Align2::LEFT_CENTER,
            editor.algorithm.label(),
            egui::FontId::monospace(13.0),
            if control_on {
                alpha.focus.color
            } else {
                alpha.ink.color
            },
        );
        painter.text(
            controls.left_center() + egui::vec2(12.0, 10.0),
            egui::Align2::LEFT_CENTER,
            editor.control_text(),
            egui::FontId::monospace(10.0),
            alpha.edge.color,
        );
        painter.text(
            controls.right_center() - egui::vec2(8.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            "TAB REGION   / ALGORITHMS   ENTER KEEP   ESC CANCEL",
            egui::FontId::monospace(10.0),
            alpha.edge.color,
        );

        if editor.picker {
            let picker = egui::Rect::from_center_size(panel.center(), egui::vec2(250.0, 286.0));
            painter.rect_filled(picker, 0.0, alpha.surface.color);
            painter.rect_stroke(
                picker,
                0.0,
                egui::Stroke::new(2.0, alpha.ink.color),
                egui::StrokeKind::Inside,
            );
            for (index, algorithm) in plock_editor::Algorithm::ALL.iter().enumerate() {
                let row = egui::Rect::from_min_size(
                    picker.min + egui::vec2(12.0, 12.0 + index as f32 * 21.0),
                    egui::vec2(picker.width() - 24.0, 20.0),
                );
                let on = *algorithm == editor.algorithm;
                if on {
                    painter.rect_filled(row, 0.0, alpha.focus.color.gamma_multiply(0.2));
                }
                painter.text(
                    row.left_center() + egui::vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    algorithm.label(),
                    egui::FontId::monospace(12.0),
                    if on {
                        alpha.focus.color
                    } else {
                        alpha.ink.color
                    },
                );
            }
        }
    }

    /// The slice strip: a dark screen with the file's envelope, a
    /// hairline at every cut, and the chosen cut washed and numbered.
    /// Without the file yet, the cuts alone on an empty screen.
    fn draw_slice_strip(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        slice: SliceRow,
        device: Option<&crate::sequencing::Device>,
    ) {
        let alpha = self.alphabet();
        let mut marks = Vec::new();
        circuit::panel_variant(
            &mut marks,
            strip,
            Some(alpha.ground.color),
            alpha.surface.color,
            Some((Weight::Hair, alpha.edge.color)),
            2,
        );
        painter.extend(marks);
        let wave = strip.shrink2(egui::vec2(4.0, 4.0));
        let count = slice.count.max(1);
        // The cuts as fractions: the authored table, or the grid the
        // knob will lay.
        let cuts: Vec<f64> = match device {
            Some(device) if !device.slices.is_empty() => device.slices.clone(),
            _ => (0..count).map(|i| i as f64 / count as f64).collect(),
        };
        let x_of = |at: f64| wave.left() + at as f32 * wave.width();
        let chosen = usize::from(slice.standing()).saturating_sub(1);
        if let Some(from) = cuts.get(chosen) {
            let to = cuts.get(chosen + 1).copied().unwrap_or(1.0);
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x_of(*from), strip.top() + 1.0),
                    egui::pos2(x_of(to), strip.bottom() - 1.0),
                ),
                0.0,
                alpha.focus.color.gamma_multiply(0.12),
            );
        }
        let file = self.sample_data.as_ref().filter(|data| {
            device.and_then(|device| device.sample.as_deref()) == Some(data.path.as_path())
        });
        let mid = wave.center().y;
        let half = wave.height() * 0.5;
        match file {
            Some(data) => {
                let columns = wave.width().max(1.0) as usize;
                let bins = data.peaks.columns(None, 0.0, 1.0, columns);
                let per = wave.width() / columns as f32;
                for (i, bin) in bins.iter().enumerate() {
                    let x = wave.left() + (i as f32 + 0.5) * per;
                    let reach = bin.max.abs().max(bin.min.abs()).clamp(0.0, 1.0) * half;
                    let rms = bin.rms.clamp(0.0, 1.0) * half;
                    painter.line_segment(
                        [egui::pos2(x, mid - reach), egui::pos2(x, mid + reach)],
                        egui::Stroke::new(1.0, alpha.edge.color),
                    );
                    if rms >= 0.5 {
                        painter.line_segment(
                            [egui::pos2(x, mid - rms), egui::pos2(x, mid + rms)],
                            egui::Stroke::new(per.max(1.0), alpha.ink.color),
                        );
                    }
                }
            }
            None => {
                painter.line_segment(
                    [egui::pos2(wave.left(), mid), egui::pos2(wave.right(), mid)],
                    egui::Stroke::new(1.0, alpha.edge.color),
                );
            }
        }
        for (index, at) in cuts.iter().enumerate() {
            let x = x_of(*at);
            let on = index == chosen;
            painter.line_segment(
                [
                    egui::pos2(x, strip.top() + 1.0),
                    egui::pos2(x, strip.bottom() - 1.0),
                ],
                egui::Stroke::new(
                    1.0,
                    if on {
                        alpha.focus.color
                    } else {
                        alpha.ink.color
                    },
                ),
            );
        }
        painter.text(
            egui::pos2(wave.left() + 3.0, wave.top()),
            egui::Align2::LEFT_TOP,
            format!("S{:02}", chosen + 1),
            egui::FontId::monospace(10.0),
            alpha.focus.color,
        );
    }

    /// One slider: the parameter's name, its range as a rail with the
    /// knob's position marked hollow, and — when the trig holds a lock
    /// — the lock marked solid in the live ink with the span from knob
    /// to lock drawn heavy, so the override reads as a DISTANCE from
    /// the setting and not merely as a second dot. The value at the
    /// right is the lock's when there is one, the knob's when not.
    fn draw_lock_row(&self, painter: &egui::Painter, row: egui::Rect, lock: &LockRow, on: bool) {
        let alpha = self.alphabet();
        let ink = if on {
            alpha.focus.color
        } else {
            alpha.ink.color
        };
        painter.text(
            egui::pos2(row.left() + 14.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            if lock.prefix.is_empty() {
                lock.label.name.to_uppercase()
            } else {
                format!("{} {}", lock.prefix, lock.label.name)
                    .to_uppercase()
                    .chars()
                    .take(15)
                    .collect()
            },
            egui::FontId::monospace(12.0),
            ink,
        );
        let y = row.center().y;
        let rail_l = row.left() + 128.0;
        let rail_r = row.right() - 92.0;
        if rail_r - rail_l < 20.0 {
            return;
        }
        let at = |value: f32| egui::pos2(rail_l + (rail_r - rail_l) * lock.fraction(value), y);
        let mut marks = Vec::new();
        circuit::trace(
            &mut marks,
            &[egui::pos2(rail_l, y), egui::pos2(rail_r, y)],
            Weight::Hair,
            alpha.edge.color,
        );
        circuit::pad(
            &mut marks,
            at(lock.knob),
            circuit::PAD - 1.0,
            alpha.edge.color,
            false,
        );
        if let Some(held) = lock.lock {
            circuit::trace(
                &mut marks,
                &[at(lock.knob), at(held)],
                Weight::Heavy,
                alpha.live.color,
            );
            circuit::pad(&mut marks, at(held), circuit::PAD, alpha.live.color, true);
        }
        painter.extend(marks);
        let (value, value_ink) = match lock.lock {
            Some(held) => (held, alpha.live.color),
            None => (lock.knob, alpha.edge.color),
        };
        painter.text(
            egui::pos2(row.right() - 4.0, y),
            egui::Align2::RIGHT_CENTER,
            chain::format_param(lock.def, lock.label, value),
            egui::FontId::monospace(11.0),
            value_ink,
        );
    }

    /// The tray with nothing in it: the deck's dormant face. A sigil
    /// wheel, two register columns, a spiral and the cosmological dial,
    /// all at the structure rung — present, and saying nothing, the way
    /// a shrine is carved before it lights.
    fn draw_quiet_tray(&self, painter: &egui::Painter, tray: egui::Rect) {
        let edge = self.alphabet().edge.color;
        let ground = self.alphabet().ground.color;
        kit::cached(
            painter,
            egui::Id::new("stage-quiet-tray"),
            tray,
            (edge, ground),
            |out| {
                let m = design::px(design::space::ROOM);
                let plaque = tray.shrink(m);
                if plaque.height() < 40.0 {
                    return;
                }
                // The plaque is the tray's own casing while nothing is in
                // it, so it carries the frame weight rather than a rule's.
                circuit::panel_frame_variant(out, plaque, Weight::Heavy, edge, 2);
                let c = plaque.center();
                let side = plaque.height() * 0.7;
                Sign::Dipper.paint(
                    out,
                    egui::Rect::from_center_size(c, egui::Vec2::splat(side)),
                    Weight::Heavy,
                    edge,
                );
                let mut rng = kit::Rng::seeded("quiet-tray");
                let unit = 8.0;
                for row in 0..3 {
                    let y = plaque.min.y + m + row as f32 * (unit + 2.0);
                    circuit::binary(
                        out,
                        egui::pos2(plaque.min.x + m, y),
                        unit,
                        rng.next_u64() as u32,
                        16,
                        edge,
                    );
                }
                circuit::rail(
                    out,
                    egui::pos2(plaque.max.x - m - 140.0, plaque.max.y - m),
                    egui::pos2(plaque.max.x - m, plaque.max.y - m),
                    &[0.0, 0.5, 1.0],
                    edge,
                );
            },
        );
    }

    /// The clip tray. The sequencer draws the clip in view — the stage
    /// builds what it reads (notes resolved against the key, the track's
    /// lens) — and has the keys only while the cursor is inside. While
    /// focus is elsewhere the tray is drawn and then veiled, so the
    /// sequencer's own white stays below the one focus-bright thing on
    /// the screen, and lands whatever it asked for on the pattern.
    /// The chain band: the addressed track's devices, in signal order,
    /// each carrying its whole parameter table as a scrolling list.
    fn draw_chain(&self, painter: &egui::Painter, tray: egui::Rect, phase: Phase) {
        let Some(lattice) = self.chain.as_ref() else {
            return;
        };
        let Some(track) = self.addressed_track() else {
            return;
        };
        let columns = chain::band(&self.song, track);
        if columns.is_empty() {
            return;
        }
        let margin = design::px(design::space::ROOM);
        let gap = column_gap();
        let head_h = CHAIN_HEAD_H;
        let pitch = CHAIN_PITCH;
        let body_top = tray.min.y + head_h;
        let rows_shown = Self::chain_capacity(tray);
        let _ = chain::rows_that_fit(tray.max.y - margin - body_top, pitch);
        let cursor = lattice.cursor();

        // The band is a rail of pieces of two kinds: cards, which stand
        // apart by the column gap, and the strip's sections, which mate
        // — a section's tongue lies in the next section's notch, so two
        // sections take no gap between them.
        let widths: Vec<f32> = columns
            .iter()
            .map(|column| column.section.map_or(CHAIN_W, strip::width_of))
            .collect();
        // Two pieces mate only when they stand on the SAME rail: a
        // channel's sections are one run, its group bus's another, the
        // mix's another. Where the band crosses from one rail to the
        // next it opens a gap, and the pair crosses it as a visible
        // cable with the rail's name engraved over it — because that
        // crossing is a real thing about the desk, not a seam to hide.
        // A return mates with nothing at all: it is a parallel path.
        let mates = |i: usize| -> bool {
            i + 1 < columns.len()
                && columns[i].section.is_some()
                && columns[i + 1].section.is_some()
                && columns[i].lane == columns[i + 1].lane
        };
        // Where the band crosses rails it opens the wider gap, so the
        // cable and the rail's name have room to be seen.
        let crossing = |i: usize| -> bool {
            i + 1 < columns.len()
                && columns[i].section.is_some()
                && columns[i + 1].section.is_some()
                && columns[i].lane != columns[i + 1].lane
                && columns[i].lane.in_series()
                && columns[i + 1].lane.in_series()
        };
        let step = |i: usize| -> f32 {
            widths[i]
                + if mates(i) {
                    0.0
                } else if crossing(i) {
                    RAIL_GAP
                } else {
                    gap
                }
        };
        let avail = tray.width() - margin * 2.0;

        // The rail scrolls so the cursor's piece is on screen: the first
        // piece shown is the earliest from which the cursor's still fits.
        let cursor_col = cursor.map_or(0, |(col, _)| col).min(columns.len() - 1);
        let mut first = 0;
        loop {
            let mut x = 0.0;
            let mut fits = false;
            for i in first..=cursor_col {
                if x + widths[i] <= avail {
                    if i == cursor_col {
                        fits = true;
                    }
                    x += step(i);
                } else {
                    break;
                }
            }
            if fits || first >= cursor_col {
                break;
            }
            first += 1;
        }
        let mut layout: Vec<(usize, egui::Rect)> = Vec::new();
        let mut x = tray.min.x + margin;
        for i in first..columns.len() {
            if x + widths[i] > tray.max.x - margin + 0.5 {
                break;
            }
            layout.push((
                i,
                egui::Rect::from_min_max(
                    egui::pos2(x, tray.top()),
                    egui::pos2(x + widths[i], tray.bottom() - margin - LOOM_H),
                ),
            ));
            x += step(i);
        }
        if layout.is_empty() {
            return;
        }

        // The signal between cards, and from the last card into the
        // first piece's notch. Between two pieces there is no trace to
        // draw: they are joined. A bypassed card bends the trace upward
        // before it arrives, as it always did.
        let signal_y = tray.top() + head_h - 5.0;
        let sounding = self.playing_on(track).is_some();
        for pair in layout.windows(2) {
            let (left_index, left_rect) = pair[0];
            let (right_index, right_rect) = pair[1];
            if columns[left_index].section.is_some() && columns[right_index].section.is_some() {
                let (left_lane, right_lane) = (columns[left_index].lane, columns[right_index].lane);
                if left_lane == right_lane {
                    // Joined. There is nothing between them to draw.
                    continue;
                }
                if !left_lane.in_series() || !right_lane.in_series() {
                    // A return is reached by its cable, not by the rail.
                    continue;
                }
                self.draw_rail_crossing(
                    painter, left_rect, right_rect, right_lane, sounding, phase,
                );
                continue;
            }
            let from = egui::pos2(left_rect.right(), signal_y);
            let right_is_piece = columns[right_index].section.is_some();
            let to = if right_is_piece {
                egui::pos2(
                    right_rect.left() + strip::TONGUE,
                    strip::joint_y(right_rect),
                )
            } else {
                egui::pos2(right_rect.left(), signal_y)
            };
            let path = if columns[right_index].bypassed && !right_is_piece {
                let lift = gap.min(8.0);
                vec![
                    from,
                    egui::pos2(from.x + lift, signal_y - lift),
                    egui::pos2(to.x - lift, signal_y - lift),
                    to,
                ]
            } else {
                circuit::elbow(from, to)
            };
            let mut shapes = Vec::new();
            circuit::trace(&mut shapes, &path, Weight::Hair, self.alphabet().edge.color);
            if sounding && phase.rolling {
                circuit::dashes(
                    &mut shapes,
                    &path,
                    phase.dash(),
                    Weight::Heavy,
                    self.alphabet().live_dim.color,
                );
            }
            painter.extend(shapes);
        }

        // The pieces: every body first, so a tongue laid afterwards lies
        // in its neighbour's notch rather than under it.
        let pieces: Vec<(strip::Piece, &chain::Column)> = layout
            .iter()
            .filter_map(|(i, rect)| {
                columns[*i].section.map(|kind| {
                    (
                        strip::Piece {
                            index: *i,
                            rect: *rect,
                            kind,
                            // A return is off the rail, so it wears a
                            // notch for its send cable and leaves by a
                            // pad rather than by a tongue.
                            notch: !columns[*i].lane.in_series()
                                || (*i > 0 && columns[*i - 1].section.is_some()),
                            tongue: mates(*i),
                        },
                        &columns[*i],
                    )
                })
            })
            .collect();
        for (piece, column) in &pieces {
            self.draw_piece_body(painter, *piece, column);
        }
        let level = self
            .meters
            .readings()
            .get(track)
            .map(|reading| reading.level.peak());
        for (piece, column) in &pieces {
            self.draw_piece_face(
                painter,
                *piece,
                column,
                cursor,
                self.chain_offset,
                rows_shown,
                sounding,
                phase,
                level,
            );
        }
        for (index, rect) in &layout {
            if columns[*index].section.is_some() {
                continue;
            }
            self.draw_chain_card(
                painter,
                *rect,
                &columns[*index],
                *index,
                cursor,
                self.chain_offset,
                rows_shown,
                head_h,
                pitch,
            );
        }
        self.draw_loom(
            painter,
            egui::Rect::from_min_max(
                egui::pos2(tray.left() + margin, tray.bottom() - margin - LOOM_H),
                egui::pos2(tray.right() - margin, tray.bottom() - margin),
            ),
            track,
            &columns,
            &layout,
            sounding,
            phase,
        );
    }

    /// Where the band crosses from one rail of the desk to the next:
    /// the channel's last section into its group bus, the bus into the
    /// mix.
    ///
    /// The pair crosses a real gap, and the rail it is arriving on is
    /// named climbing beside it. This is the one place on the band
    /// where a card does NOT plug into its neighbour, and it is the
    /// place where the signal stops being one track's and becomes the
    /// desk's — so the eye is told, rather than left to guess from a
    /// change of seal.
    fn draw_rail_crossing(
        &self,
        painter: &egui::Painter,
        left: egui::Rect,
        right: egui::Rect,
        lane: chain::Lane,
        sounding: bool,
        phase: Phase,
    ) {
        let alpha = self.alphabet();
        let jy = strip::joint_y(right);
        let from_x = left.right();
        let to_x = right.left() + strip::TONGUE;
        let mut shapes = Vec::new();
        for dy in [-4.0, 4.0] {
            let path = [egui::pos2(from_x, jy + dy), egui::pos2(to_x, jy + dy)];
            circuit::trace(&mut shapes, &path, Weight::Hair, alpha.edge.color);
            if sounding && phase.rolling {
                circuit::dashes(
                    &mut shapes,
                    &path,
                    phase.dash(),
                    Weight::Heavy,
                    alpha.live_dim.color,
                );
            }
            circuit::pad(
                &mut shapes,
                egui::pos2(from_x, jy + dy),
                circuit::PAD - 1.0,
                alpha.edge.color,
                true,
            );
        }
        // A hairline dropped the height of the band marks the seam, so
        // the run of pieces reads as two runs rather than as one long
        // one with a gap in it.
        let seam = (from_x + to_x) * 0.5;
        circuit::trace(
            &mut shapes,
            &[
                egui::pos2(seam, right.top() + strip::HEAD_H),
                egui::pos2(seam, jy + 14.0),
            ],
            Weight::Hair,
            alpha.edge.color.gamma_multiply(0.5),
        );
        painter.extend(shapes);
        if let Some(name) = lane.seal(&self.song) {
            block::paint_vertical(
                painter,
                egui::pos2(seam - 5.0, right.bottom() - 6.0),
                block::unit::MICRO,
                &name,
                alpha.edge.color,
            );
        }
    }

    /// The LOOM: the two sends leaving the channel's OUT for their
    /// returns, and the two returns landing back in the mix.
    ///
    /// A list of devices left to right can only say that the signal
    /// goes one way. It does not: OUT taps a share of the channel into
    /// TAPE and into SHADOW, and what those two make comes back into
    /// the mix beside everything else. So the sends are drawn as what
    /// they are — cables, running the length of the desk along the
    /// band's foot, out on the upper pair and home on the lower, each
    /// return its own colour. A cable is as bright as its send is open,
    /// so a closed send is a cable that is plainly not carrying
    /// anything, and the dashes on it travel with the beat.
    #[allow(clippy::too_many_arguments)]
    fn draw_loom(
        &self,
        painter: &egui::Painter,
        loom: egui::Rect,
        track: usize,
        columns: &[chain::Column],
        layout: &[(usize, egui::Rect)],
        sounding: bool,
        phase: Phase,
    ) {
        use crate::params::console::out as p;
        if !loom.is_positive() {
            return;
        }
        let alpha = self.alphabet();
        let seen = |at: usize| layout.iter().find(|(i, _)| *i == at).map(|(_, r)| *r);
        let column_of =
            |want: &dyn Fn(&chain::Column) -> bool| columns.iter().position(|c| want(c));

        // The three places a cable touches: where the send leaves, where
        // it lands, and where the return comes home.
        let out_col = column_of(&|column: &chain::Column| {
            column.lane == chain::Lane::Channel
                && column.section == Some(crate::console::SectionKind::Out)
        });
        let mix_col = column_of(&|column: &chain::Column| column.lane == chain::Lane::Mix);
        let out_rect = out_col.and_then(seen);
        let mix_rect = mix_col.and_then(seen);

        // How open each send is: what OUT measured of itself last block
        // when the engine is running, and what the hand set when it is
        // not.
        let sends = out_col
            .and_then(|col| chain::device_at(&self.song, track, col))
            .map(|id| {
                let said = self.telemetry(id);
                let set = self.song.device(id);
                let value = |param: u32| set.map_or(0.0, |device| device.value(param)) / 100.0;
                if said.bands[1] > 0.0 || said.bands[2] > 0.0 {
                    [said.bands[1] / 100.0, said.bands[2] / 100.0]
                } else {
                    [value(p::SEND_TAPE), value(p::SEND_SHADOW)]
                }
            })
            .unwrap_or([0.0, 0.0]);

        let mut shapes = Vec::new();
        for index in 0..2 {
            let ink = RETURN_INK[index];
            let open = sends[index].clamp(0.0, 1.0);
            let ret_col =
                column_of(&|column: &chain::Column| column.lane == chain::Lane::Return(index));
            let ret_rect = ret_col.and_then(seen);
            // Out on the upper pair, home on the lower, each return
            // keeping its own line so two cables never read as one.
            let out_y = loom.top() + 3.0 + index as f32 * 3.0;
            let home_y = loom.bottom() - 3.0 - index as f32 * 3.0;
            let carrying = open > 0.005;
            let cable = if carrying {
                tint(ink, 0.35 + 0.65 * open)
            } else {
                alpha.edge.color.gamma_multiply(0.55)
            };

            // The send: down out of OUT's foot, along the loom, up into
            // the return's notch.
            let from_x = out_rect.map_or(loom.left(), |rect| rect.center().x + 10.0 * index as f32);
            let to_x = ret_rect.map_or(loom.right(), |rect| rect.left() + strip::TONGUE);
            let send = vec![
                egui::pos2(from_x, out_rect.map_or(out_y, |rect| rect.bottom())),
                egui::pos2(from_x, out_y),
                egui::pos2(to_x, out_y),
                egui::pos2(to_x, ret_rect.map_or(out_y, |rect| strip::joint_y(rect))),
            ];
            circuit::trace(&mut shapes, &send, Weight::Hair, cable);
            if carrying && sounding && phase.rolling {
                circuit::dashes(&mut shapes, &send, phase.dash(), Weight::Heavy, ink);
            }
            if let Some(rect) = out_rect {
                // The tap: a pad on the channel's foot that fills as the
                // send opens. It is the send, not a picture of it.
                circuit::pad(
                    &mut shapes,
                    egui::pos2(from_x, rect.bottom()),
                    circuit::PAD,
                    cable,
                    carrying,
                );
            }

            // The way home: out of the return's right edge, along the
            // loom, up into the mix's notch.
            let home_from = ret_rect.map_or(loom.right(), |rect| rect.right());
            let home_to = mix_rect.map_or(loom.left(), |rect| rect.left() + strip::TONGUE);
            let home = vec![
                egui::pos2(
                    home_from,
                    ret_rect.map_or(home_y, |rect| strip::joint_y(rect)),
                ),
                egui::pos2(home_from + 6.0, home_y),
                egui::pos2(home_to, home_y),
                egui::pos2(
                    home_to,
                    mix_rect.map_or(home_y, |rect| strip::joint_y(rect)),
                ),
            ];
            circuit::trace(&mut shapes, &home, Weight::Hair, cable);
            if carrying && sounding && phase.rolling {
                circuit::dashes(&mut shapes, &home, phase.dash(), Weight::Heavy, ink);
            }
            if let Some(rect) = ret_rect {
                circuit::pad(
                    &mut shapes,
                    egui::pos2(rect.right(), strip::joint_y(rect)),
                    circuit::PAD - 1.0,
                    cable,
                    carrying,
                );
            }
        }
        painter.extend(shapes);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_chain_card(
        &self,
        painter: &egui::Painter,
        card: egui::Rect,
        column: &chain::Column,
        index: usize,
        cursor: Option<(usize, usize)>,
        row_offset: usize,
        rows_shown: usize,
        head_h: f32,
        pitch: f32,
    ) {
        let alpha = self.alphabet();
        let head = egui::Rect::from_min_size(card.min, egui::vec2(card.width(), head_h));
        let body_top = head.bottom();
        let row_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        let family_ink = if column.bypassed {
            alpha.edge.color
        } else {
            alpha.ink.color
        };
        kit::cached(
            painter,
            egui::Id::new(("stage-chain-card", index)),
            card,
            (
                alpha.surface.color,
                alpha.ground.color,
                alpha.edge.color,
                family_ink,
            ),
            |out| {
                circuit::panel_variant(
                    out,
                    card,
                    Some(alpha.surface.color),
                    alpha.ground.color,
                    Some((Weight::Hair, alpha.edge.color)),
                    index as u8,
                );
                circuit::trace(
                    out,
                    &[
                        egui::pos2(card.left() + circuit::CHAMFER, head.bottom()),
                        egui::pos2(card.right() - circuit::CHAMFER, head.bottom()),
                    ],
                    Weight::Hair,
                    alpha.edge.color,
                );
                for point in [
                    egui::pos2(card.center().x, card.top()),
                    egui::pos2(card.center().x, card.bottom()),
                    egui::pos2(card.left(), head.bottom() - 5.0),
                    egui::pos2(card.right(), head.bottom() - 5.0),
                ] {
                    circuit::pad(out, point, circuit::PAD, family_ink, true);
                }
                Sign::Seal(browser::family_mark(column.family)).paint(
                    out,
                    egui::Rect::from_center_size(
                        egui::pos2(head.left() + 16.0, head.center().y),
                        egui::Vec2::splat(19.0),
                    ),
                    Weight::Hair,
                    family_ink,
                );
                circuit::pad(
                    out,
                    egui::pos2(head.right() - 11.0, head.center().y),
                    circuit::PAD + 1.0,
                    family_ink,
                    !column.bypassed,
                );
            },
        );

        // The full catalog name remains the column's semantic title; the
        // header cuts its stable target prefix. That address is the terse
        // machine name the narrow card can carry at the real block-face
        // size (POLY, SAT, REVERB), rather than shrinking prose into an
        // unreadable seven-pixel imitation of the face.
        let title = column.code.to_ascii_uppercase();
        block::paint(
            painter,
            egui::Id::new(("stage-chain-title", index)),
            egui::pos2(head.left() + 30.0, head.top() + 5.0),
            egui::Align2::LEFT_TOP,
            block::unit::MICRO,
            &title,
            family_ink,
        );
        if let Some(sample) = &column.sample {
            painter.text(
                egui::pos2(head.left() + 30.0, head.bottom() - 5.0),
                egui::Align2::LEFT_BOTTOM,
                fit_cells(sample, 24),
                row_font.clone(),
                family_ink,
            );
        }

        // The card's family word climbs its outer rail in the typewriter
        // hand, separate from the parameter-family signs on the next rail.
        let family_word = device_family_word(column.family);
        let galley =
            painter.layout_no_wrap(family_word.to_owned(), row_font.clone(), alpha.edge.color);
        painter.add(egui::Shape::Text(
            egui::epaint::TextShape::new(
                egui::pos2(card.left() + 5.0, card.bottom() - 8.0),
                galley,
                alpha.edge.color,
            )
            .with_angle(-core::f32::consts::FRAC_PI_2),
        ));

        let visible = row_offset..(row_offset + rows_shown).min(column.rows.len());
        for (family, run) in chain::family_runs(&column.rows) {
            let start = run.start.max(visible.start);
            let end = run.end.min(visible.end);
            if start >= end {
                continue;
            }
            let first_line = start - row_offset;
            let last_line = end - 1 - row_offset;
            let x = card.left() + 23.0;
            let y0 = body_top + first_line as f32 * pitch + pitch * 0.5;
            let y1 = body_top + last_line as f32 * pitch + pitch * 0.5;
            let mut shapes = Vec::new();
            circuit::rail(
                &mut shapes,
                egui::pos2(x, y0),
                egui::pos2(x, y1.max(y0 + 1.0)),
                &[0.0, 1.0],
                alpha.edge.color,
            );
            Sign::Family(family).paint(
                &mut shapes,
                egui::Rect::from_center_size(egui::pos2(x, y0), egui::Vec2::splat(11.0)),
                Weight::Hair,
                alpha.edge.color,
            );
            for shape in shapes {
                painter.add(shape);
            }
        }

        let cell_w = painter
            .layout_no_wrap("M".to_owned(), row_font.clone(), alpha.ink.color)
            .rect
            .width()
            .max(1.0);
        for line in 0..rows_shown {
            let Some(row) = column.rows.get(row_offset + line) else {
                break;
            };
            // The row: in from the family rail, and short of the right
            // edge by a real margin, so the value never touches the
            // casing and the cursor's brackets have room to sit.
            let rect = egui::Rect::from_min_size(
                egui::pos2(card.left() + 36.0, body_top + line as f32 * pitch),
                egui::vec2(card.width() - 48.0, pitch),
            );
            let on_row = cursor == Some((index, row_offset + line));
            if on_row {
                // The cursor row is the brightest thing on the card:
                // ground-coloured words on the focus ink, so the row
                // under the hand is never the hardest one to read.
                painter.rect_filled(rect, 0.0, alpha.focus.color);
                crate::ui::nav_cursor::claim(
                    painter,
                    ("stage-chain-row-cursor", index, row_offset + line),
                    rect,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Surface,
                    alpha.ink.color,
                );
            }
            // Read at the ink, not the edge: a card is a table to be
            // read, and a table in the structure rung is a table you
            // lean into. A value moved off its default steps up once
            // more, to the focus ink, so the edits are found at a glance.
            let value_ink = if on_row {
                alpha.ground.color
            } else if row.edited {
                alpha.focus.color
            } else {
                alpha.ink.color
            };
            let name_ink = if on_row {
                alpha.ground.color
            } else {
                alpha.ink.color
            };
            let value_w = painter
                .layout_no_wrap(row.value.clone(), row_font.clone(), value_ink)
                .rect
                .width();
            // A value column wide enough for the longest word a row can
            // say, so the gauges line up down the card instead of
            // wandering with each value's length.
            let value_col = (cell_w * 8.0).max(value_w);
            let gauge_w = 44.0;
            let value_x = rect.right();
            let gauge = egui::Rect::from_center_size(
                egui::pos2(value_x - value_col - gauge_w * 0.5 - 10.0, rect.center().y),
                egui::vec2(gauge_w, 7.0),
            );
            let name_room = (gauge.left() - rect.left() - 8.0).max(cell_w);
            let name_cells = (name_room / cell_w).floor().max(1.0) as usize;
            painter.text(
                egui::pos2(rect.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                fit_cells(&row.name, name_cells),
                row_font.clone(),
                name_ink,
            );
            let mut shapes = Vec::new();
            if row.choices > 0 {
                circuit::choice_bar(
                    &mut shapes,
                    gauge,
                    row.choices,
                    row.choice,
                    value_ink,
                    if on_row {
                        alpha.surface.color
                    } else {
                        alpha.edge.color
                    },
                );
            } else {
                circuit::tick_bar(
                    &mut shapes,
                    gauge,
                    12,
                    row.place,
                    value_ink,
                    if on_row {
                        alpha.surface.color
                    } else {
                        alpha.edge.color
                    },
                    true,
                );
            }
            for shape in shapes {
                painter.add(shape);
            }
            painter.text(
                egui::pos2(value_x, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &row.value,
                row_font.clone(),
                value_ink,
            );
        }

        if column.rows.len() > row_offset + rows_shown {
            let mut shapes = Vec::new();
            circuit::annotation_arrow(
                &mut shapes,
                egui::pos2(card.center().x, card.bottom() - 2.0),
                egui::pos2(card.center().x, card.bottom() + 8.0),
                alpha.ink.color,
            );
            for shape in shapes {
                painter.add(shape);
            }
        }
    }

    fn draw_clip(&mut self, ui: &mut egui::Ui, tray: egui::Rect) -> Option<egui::Rect> {
        let Some(shown) = self.clip_in_view() else {
            self.draw_quiet_tray(ui.painter(), tray);
            return None;
        };
        let Some(pattern) = self.song.pattern(shown.pattern) else {
            return None;
        };
        let lens_name = match self.entry_mode(shown) {
            midi_typing::EntryMode::Degree { .. } => "degrees",
            midi_typing::EntryMode::Chromatic => "notes",
        };
        let lens_view = lens::LensView::resolve(lens_name, &self.song.key, &|_| None);
        let notes = sequencer::note_views(pattern, &self.song.key);
        let name = pattern.name.clone();
        let clip = sequence::ClipView {
            id: shown.pattern.0,
            name: &name,
            length_ticks: sequencer::pattern_length(&self.song, shown.pattern),
            notes: &notes,
            ghosts: &[],
            slicing: self.slicing_track(shown.track),
        };
        let focused = self.inside.is_some() && self.browser.is_none();
        // While the trig menu is up the sequencer is seen and not heard
        // from: it keeps its cursor and its view, but the keys are the
        // menu's, so its grammar must not consume them underneath.
        let keys = focused && self.trig_menu.is_none() && self.plock_editor.is_none();

        // The sequencer sits at the tray's left, inside the field's own
        // margin, and no wider than its natural width: a grid that
        // stretched with the window would be a grid the eye relearns.
        let margin = design::px(design::space::ROOM);
        let area = egui::Rect::from_min_max(
            egui::pos2(tray.min.x + margin, tray.min.y),
            egui::pos2(
                (tray.max.x - margin).min(tray.min.x + margin + CLIP_W_MAX),
                tray.max.y,
            ),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).id_salt("stage-clip"));
        // Computed BEFORE the panel borrows the sequencer: the moment is
        // a fact about the song and the transport, not about the editor.
        let playhead = self.playhead(shown);
        let outcome = self.sequencer.show(
            &mut child,
            keys,
            grammar::Voice {
                sentence: &mut self.sentence,
                registers: &mut self.registers,
            },
            self.entered_pitch.take(),
            Some(clip),
            &lens_view,
            self.polarity,
            playhead,
        );
        let anchor = outcome.cursor_rect;
        if focused {
            let edited = !outcome.intents.is_empty();
            self.apply_sequence(shown.pattern, &outcome.intents);
            if edited {
                if self.entry_held {
                    self.entry_transaction = true;
                } else {
                    // Sequencer edits are normally produced while drawing,
                    // outside `Stage::apply`; give each completed command
                    // the same history boundary as a stage intent.
                    self.settle();
                }
            }
            // The level under the cursor mirrors the sequencer's step, so
            // the ancestry strip tells the truth about where the performer is.
            if let Some(tick) = outcome.cursor_tick {
                let step = (tick / PATTERN_STEP_TICKS).min(PATTERN_STEPS - 1);
                if let FocusScope::Grid(grid) = self.focus.active_mut() {
                    grid.set_cursor(step % PATTERN_COLS, step / PATTERN_COLS);
                }
            }
        } else {
            // A veil, not a repaint: the tray keeps every mark it would
            // have, one step down in value. Exactly one thing on the
            // screen is focus-bright, and while the cursor is on the
            // session that thing is the cursor.
            ui.painter().rect_filled(area, 0.0, self.veil());
            // A click in the tray is the pointer's way in, the same
            // road Enter takes.
            if outcome.claim_focus && self.browser.is_none() {
                let _ = self.apply(StageIntent::Enter);
            }
        }
        anchor
    }

    fn draw_field(&self, painter: &egui::Painter, avail: egui::Rect, phase: Phase) {
        // The field turned over: the song's arrangement in the session's
        // place. The mixer keeps its own picture over either.
        if self.song_view && !self.mixing {
            self.draw_song(painter, avail, phase);
            return;
        }
        if let Some((lattice, cursor_shade)) = self.session_lattice() {
            if !self.mixing {
                self.draw_board(painter, avail, phase);
            }
            self.draw_tracks(painter, avail, lattice, cursor_shade);
            self.draw_master(painter, avail, cursor_shade, phase);
            // One lattice, two contents. The heads above are the same
            // heads either way — only what hangs beneath them changes.
            if self.mixing {
                self.draw_mixer(painter, avail, lattice, cursor_shade);
            } else {
                self.draw_scenes(painter, avail, lattice, cursor_shade, phase);
            }
            return;
        }

        let FocusScope::Grid(active) = self.focus.active() else {
            return;
        };
        let cols = active.cols() as f32;
        let rows = active.rows() as f32;

        // The grid sits centred at a fixed aspect: cells are square, the
        // gap scales with the cell, and nothing about the layout ever
        // depends on where focus is — geometry is constant by rule.
        let cell = ((avail.width() / cols).min(avail.height() / rows) * 0.82).floor();
        let gap = (cell * 0.14).floor().max(2.0);
        let span_x = cols * cell + (cols - 1.0) * gap;
        let span_y = rows * cell + (rows - 1.0) * gap;
        let origin = egui::pos2(
            (avail.center().x - span_x / 2.0).floor(),
            (avail.center().y - span_y / 2.0).floor(),
        );

        // Exactly one thing on the screen is ever FOCUS-bright. While the
        // browser holds the cursor, the field keeps a RESTING mark instead
        // — where focus will land when it comes back, not where it is.
        let cursor_shade = if self.browser.is_some() {
            self.resting()
        } else {
            self.focused()
        };

        // The dormant deck: the grid sits in an octagonal shield with a pad
        // on each shoulder, at the structure rung so the squares stay the
        // subject. Carved before it lights, the way a shrine is.
        {
            let edge = self.alphabet().edge.color;
            let span = egui::vec2(span_x, span_y);
            kit::cached(
                painter,
                egui::Id::new("stage-field-sheet"),
                avail,
                (edge, span.x as i32, span.y as i32),
                |out| {
                    let shield = egui::Rect::from_center_size(
                        avail.center(),
                        span + egui::Vec2::splat(design::px(design::space::VAST) * 2.0),
                    );
                    let shield = shield.intersect(avail.shrink(design::px(design::space::SNUG)));
                    circuit::panel_frame_variant(out, shield, Weight::Heavy, edge, 1);
                },
            );
        }

        let (focus_col, focus_row) = active.cursor();
        for row in 0..active.rows() {
            for col in 0..active.cols() {
                let rect = egui::Rect::from_min_size(
                    origin + egui::vec2(col as f32 * (cell + gap), row as f32 * (cell + gap)),
                    egui::vec2(cell, cell),
                );
                let shade = if (col, row) == (focus_col, focus_row) {
                    cursor_shade
                } else {
                    self.square()
                };
                painter.rect_filled(rect, 0.0, shade);
            }
        }

        // The absorbed keystroke, present only on frames where a refusal
        // happened, in the field's own geometry.
        let field = egui::Rect::from_min_size(origin, egui::vec2(span_x, span_y));
        let focused = egui::Rect::from_min_size(
            origin
                + egui::vec2(
                    focus_col as f32 * (cell + gap),
                    focus_row as f32 * (cell + gap),
                ),
            egui::vec2(cell, cell),
        );
        crate::ui::nav_cursor::claim(
            painter,
            "stage-nested-grid-cursor",
            focused,
            crate::ui::nav_cursor::Kind::Cell,
            crate::ui::nav_cursor::Layer::Surface,
            self.alphabet().ink.color,
        );
        self.draw_grid_refusal(painter, field, focused, gap.max(4.0));
    }

    /// The absorbed keystroke in a grid's geometry. Each limit refuses in
    /// its own shape: an edge marks its side, the root marks the whole
    /// field, the depth cap marks the square that would not open.
    fn draw_grid_refusal(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        focused: egui::Rect,
        inset: f32,
    ) {
        match self.refusal.map(|refusal| refusal.reason) {
            Some(RefusalReason::Edge(step)) => {
                let (a, b) = match step {
                    Step::Up => (
                        egui::pos2(field.left(), field.top() - inset),
                        egui::pos2(field.right(), field.top() - inset),
                    ),
                    Step::Down => (
                        egui::pos2(field.left(), field.bottom() + inset),
                        egui::pos2(field.right(), field.bottom() + inset),
                    ),
                    Step::Left => (
                        egui::pos2(field.left() - inset, field.top()),
                        egui::pos2(field.left() - inset, field.bottom()),
                    ),
                    Step::Right => (
                        egui::pos2(field.right() + inset, field.top()),
                        egui::pos2(field.right() + inset, field.bottom()),
                    ),
                };
                painter.line_segment([a, b], egui::Stroke::new(2.0, self.refusal_ink()));
            }
            Some(RefusalReason::Shallower) => {
                painter.rect_stroke(
                    field.expand(inset),
                    0.0,
                    egui::Stroke::new(2.0, self.refusal_ink()),
                    egui::StrokeKind::Outside,
                );
            }
            Some(RefusalReason::Deeper) => {
                painter.rect_stroke(
                    focused.expand((inset * 0.5).max(2.0)),
                    0.0,
                    egui::Stroke::new(2.0, self.refusal_ink()),
                    egui::StrokeKind::Outside,
                );
            }
            // A refusal that happened in the browser has no geometry in
            // the field — it belongs to the other side of the screen, and
            // the message strip is where it is reported. An empty or
            // unavailable verb on a step is reported the same way.
            Some(RefusalReason::Empty | RefusalReason::AtTop | RefusalReason::Unavailable)
            | None => {}
        }
    }

    /// The session's powered display plane.  It is one continuous piece of
    /// dark glass, with a data spine for the scenes and one vertical conduit
    /// per track.  Heads and cells are painted over it as addressable
    /// modules, so the session reads as a machine rather than a spreadsheet.
    fn draw_board(&self, painter: &egui::Painter, field: egui::Rect, phase: Phase) {
        let tracks = self.strip_window(field);
        if tracks.is_empty() {
            return;
        }
        let rows = self.scene_window(field);
        let margin = design::px(design::space::ROOM);
        let head = Self::head_rect(field, 0);
        let seam = Self::master_rect(field).left() - column_gap() * 0.5;
        let bottom = if rows.is_empty() {
            head.bottom() + design::px(design::space::STEP)
        } else {
            scenes::slot_beneath(head, rows.len() - 1, row_gap(), section_gap()).bottom()
                + design::px(design::space::STEP)
        };
        let area = egui::Rect::from_min_max(
            egui::pos2(head.left() - ADDRESS_W, head.top()),
            egui::pos2(seam, bottom.min(field.bottom() - margin)),
        );
        if !area.is_positive() {
            return;
        }

        let cols: Vec<f32> = tracks
            .clone()
            .enumerate()
            .map(|(slot, _)| bus_x(field, slot))
            .collect();
        let row_y: Vec<f32> = rows
            .clone()
            .enumerate()
            .map(|(line, _)| {
                scenes::slot_beneath(head, line, row_gap(), section_gap())
                    .center()
                    .y
            })
            .collect();
        let alpha = self.alphabet();
        let ground = alpha.ground.color;
        let glass = alpha.well.color;
        let structure = alpha.edge.color.gamma_multiply(0.56);
        // The board's rails are the instrument's own frame. A frame is
        // not a signal, so it does not wear the sounding hue.
        let powered = alpha.edge.color.gamma_multiply(1.05);
        let spine_x = head.left() - 10.0;
        kit::cached(
            painter,
            egui::Id::new("stage-session-board"),
            area,
            (
                glass,
                structure,
                powered,
                ground,
                tracks.len(),
                rows.len(),
                tracks.start,
                rows.start,
            ),
            |out| {
                // The aperture itself: a second inset cut makes this read as
                // glass seated in a chassis, not a border around a table.
                circuit::relic_frame(out, area, glass, Weight::Heavy, structure);
                let inner = area.shrink(5.0);
                out.push(egui::Shape::closed_line(
                    circuit::relic_points(inner),
                    egui::Stroke::new(Weight::Hair.px(), powered),
                ));

                // The left-hand scene spine.  Each row branches from this
                // powered line before crossing the track conduits.
                circuit::rail_weighted(
                    out,
                    egui::pos2(spine_x, head.bottom() - 6.0),
                    egui::pos2(spine_x, area.bottom() - 9.0),
                    &[],
                    Weight::Heavy,
                    powered,
                );
                for y in &row_y {
                    let lane_top = *y - scenes::SLOT_H * 0.34;
                    let lane_bottom = *y + scenes::SLOT_H * 0.34;
                    circuit::trace(
                        out,
                        &[
                            egui::pos2(area.left() + 7.0, *y),
                            egui::pos2(spine_x, *y),
                            egui::pos2(spine_x + 6.0, lane_top),
                            egui::pos2(area.right() - 12.0, lane_top),
                        ],
                        Weight::Hair,
                        structure,
                    );
                    circuit::trace(
                        out,
                        &[
                            egui::pos2(spine_x + 6.0, lane_bottom),
                            egui::pos2(area.right() - 22.0, lane_bottom),
                            egui::pos2(area.right() - 14.0, *y),
                            egui::pos2(area.right() - 7.0, *y),
                        ],
                        Weight::Hair,
                        structure.gamma_multiply(0.76),
                    );
                    circuit::relic_node(out, egui::pos2(spine_x, *y), 4.5, powered, ground);
                }

                // Every track descends from its head through all scene
                // addresses.  The short cap above it makes the connection
                // visible even where the filled cell hides the conduit.
                for x in &cols {
                    circuit::trace(
                        out,
                        &[
                            egui::pos2(*x - 8.0, head.top() + 8.0),
                            egui::pos2(*x, head.top() + 16.0),
                            egui::pos2(*x, area.bottom() - 8.0),
                        ],
                        Weight::Hair,
                        powered,
                    );
                    circuit::pad(
                        out,
                        egui::pos2(*x, area.bottom() - 8.0),
                        circuit::PAD - 1.0,
                        powered,
                        true,
                    );
                }

                circuit::pad(
                    out,
                    egui::pos2(area.left() + 7.0, area.top() + 7.0),
                    circuit::PAD,
                    powered,
                    true,
                );
                circuit::pad(
                    out,
                    egui::pos2(area.right() - 11.0, area.bottom() - 7.0),
                    circuit::PAD,
                    powered,
                    false,
                );
            },
        );

        if phase.rolling {
            for (slot, track) in tracks.enumerate() {
                if self.playing_on(track).is_none() {
                    continue;
                }
                let x = bus_x(field, slot);
                let mut shapes = Vec::new();
                circuit::dashes(
                    &mut shapes,
                    &[
                        egui::pos2(x, head.bottom() - 5.0),
                        egui::pos2(x, area.bottom() - 8.0),
                    ],
                    phase.dash(),
                    Weight::Heavy,
                    alpha.live.color,
                );
                for shape in shapes {
                    painter.add(shape);
                }
            }
        }
    }

    /// The track strip: every track in the song, across the top of the
    /// field, one column each. Identity only — name and kind — because a
    /// surface has to say what its objects ARE before it can say what they
    /// are doing.
    fn draw_tracks(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        lattice: &FocusLattice,
        cursor_shade: egui::Color32,
    ) {
        let heads = tracks::heads(&self.song);
        if heads.is_empty() {
            return;
        }

        let gap = column_gap();
        let rows_gap = row_gap();
        let margin = design::px(design::space::ROOM);
        let pad = design::px(design::space::STEP);
        let name_font = egui::FontId::monospace(design::px(design::type_scale::BODY));
        let kind_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        let top = field.min.y + margin;

        let window = self.strip_window(field);
        let (first, last) = (window.start, window.end);
        let shown = &heads[window];

        for (slot, head) in shown.iter().enumerate() {
            let index = first + slot;
            let rect = Self::head_rect(field, slot);
            let focused = lattice.cursor() == Some((index, 0));
            let alpha = self.alphabet();
            let fill = if focused {
                cursor_shade
            } else {
                alpha.well.color
            };
            let figure_ink = if focused {
                alpha.ground.color
            } else {
                alpha.ink.color
            };
            let powered = if focused {
                alpha.ground.color
            } else {
                alpha.edge.color.gamma_multiply(1.25)
            };
            let rail_x = rect.left() + 18.0;
            let content_x = rect.left() + 36.0;
            kit::cached(
                painter,
                egui::Id::new(("stage-track-head", index)),
                rect,
                (fill, alpha.ground.color, figure_ink, powered),
                |out| {
                    circuit::relic_frame(out, rect, fill, Weight::Heavy, powered);
                    out.push(egui::Shape::closed_line(
                        circuit::relic_points(rect.shrink(4.0)),
                        egui::Stroke::new(
                            Weight::Hair.px(),
                            if focused {
                                alpha.well.color
                            } else {
                                alpha.edge.color
                            },
                        ),
                    ));

                    // The track's powered node feeds both its title plate
                    // and the conduit that continues through its scenes.
                    circuit::relic_node(
                        out,
                        egui::pos2(rail_x, rect.center().y - 2.0),
                        10.0,
                        powered,
                        figure_ink,
                    );
                    Sign::General((index % 32) as u8).paint(
                        out,
                        egui::Rect::from_center_size(
                            egui::pos2(rail_x, rect.center().y - 2.0),
                            egui::Vec2::splat(11.0),
                        ),
                        Weight::Hair,
                        figure_ink,
                    );
                    circuit::trace(
                        out,
                        &[
                            egui::pos2(content_x, rect.top() + 7.0),
                            egui::pos2(rect.right() - 26.0, rect.top() + 7.0),
                            egui::pos2(rect.right() - 20.0, rect.top() + 13.0),
                        ],
                        Weight::Hair,
                        powered,
                    );
                    circuit::pad(
                        out,
                        egui::pos2(rail_x, rect.bottom() - 1.0),
                        circuit::PAD,
                        figure_ink,
                        true,
                    );
                    circuit::pad(
                        out,
                        egui::pos2(rect.right() - 18.0, rect.bottom() - 1.0),
                        circuit::PAD,
                        figure_ink,
                        true,
                    );
                    for offset in [0.0, 5.0, 10.0] {
                        circuit::pad(
                            out,
                            egui::pos2(rect.right() - 28.0 + offset, rect.bottom() - 5.0),
                            2.0,
                            powered,
                            offset == 10.0,
                        );
                    }
                },
            );
            if focused {
                let target = if self.mixing {
                    egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(
                            rect.right(),
                            field.bottom() - design::px(design::space::ROOM),
                        ),
                    )
                } else {
                    rect
                };
                crate::ui::nav_cursor::claim(
                    painter,
                    ("stage-track-cursor", index),
                    target,
                    if self.mixing {
                        crate::ui::nav_cursor::Kind::Column
                    } else {
                        crate::ui::nav_cursor::Kind::Cell
                    },
                    crate::ui::nav_cursor::Layer::Surface,
                    self.alphabet().ink.color,
                );
            }

            // The family this track sounds, as its own mark. A chain's
            // head decides it; a track with no chain sounds the default
            // voice and says so with the same sign the browser files it
            // under, so the shape means one thing in both places.
            // The mark takes the head's top-right corner, and the name
            // is cut to leave it. A name that ran under the sigil would
            // be two things in one place, which is the failure the whole
            // ornament rule exists to avoid.
            let sigil_side = design::px(design::space::STEP);
            let sigil_room = sigil_side + pad;
            if let Some(mark) = self.track_sigil(index) {
                // The same strokes the browser files the family under,
                // drawn with the brush: the meaning is the shape's, the
                // hand is the house's.
                let cell = egui::Rect::from_center_size(
                    egui::pos2(
                        rect.max.x - pad - sigil_side / 2.0,
                        rect.min.y + pad * 0.9 + sigil_side / 2.0,
                    ),
                    egui::Vec2::splat(sigil_side),
                );
                let colour = if focused {
                    self.alphabet().well.color
                } else {
                    self.alphabet().ink.color
                };
                Sign::Seal(mark).painted(
                    painter,
                    egui::Id::new(("stage-head-mark", index)),
                    cell,
                    Weight::Hair,
                    colour,
                );
            }

            // The focused column inverts, exactly as the field's cursor
            // inverts: one signal drawn one way everywhere. Within the
            // column the name outranks the kind on both sides of the
            // inversion, so the reading order survives it.
            // Both lines carry the content rung: a head's kind and its
            // number are facts to be READ, and at the structure rung on a
            // near-black plane they were closer to the ground than to the
            // name beside them. The name still outranks the kind — by SIZE,
            // which was always doing that work as well.
            let (name_ink, kind_ink) = if focused {
                (self.alphabet().ground.color, self.alphabet().well.color)
            } else {
                (self.alphabet().ink.color, self.alphabet().ink.color)
            };
            // While this head is being renamed the letters typed so far
            // ARE its name, with a caret to say the word is not finished.
            let name = match &self.renaming {
                Some(rename) if rename.track == index => format!("{}_", rename.text),
                _ => head.name.clone(),
            };
            // Cut to leave the sigil its corner: a name that ran under
            // the mark would be two things in one place.
            let name = {
                let cell = painter
                    .layout_no_wrap("M".to_owned(), name_font.clone(), name_ink)
                    .rect
                    .width()
                    .max(1.0);
                let cells = (((rect.right() - pad - sigil_room - content_x) / cell).floor())
                    .max(1.0) as usize;
                name.chars().take(cells).collect::<String>()
            };
            painter.text(
                egui::pos2(content_x, rect.min.y + pad),
                egui::Align2::LEFT_TOP,
                name,
                name_font.clone(),
                name_ink,
            );
            painter.text(
                egui::pos2(content_x, rect.max.y - pad),
                egui::Align2::LEFT_BOTTOM,
                head.kind,
                kind_font.clone(),
                kind_ink,
            );
            // The column's address, the way the sequencer numbers its
            // rows: a track is a channel with a number before it has a
            // name, and the number is what a held key will one day say.
            // On the kind's line, not the name's: a name may run the
            // whole width, and the number must never be under it.
            let number = format!("{:02}", index + 1);
            block::paint(
                painter,
                egui::Id::new(("stage-head-number", index)),
                egui::pos2(rect.max.x - pad, rect.max.y - pad),
                egui::Align2::RIGHT_BOTTOM,
                block::unit::MICRO,
                &number,
                kind_ink,
            );
            Sign::Numeral(((index + 1) % 10) as u8).painted(
                painter,
                egui::Id::new(("stage-head-codex-number", index)),
                egui::Rect::from_center_size(
                    egui::pos2(
                        rect.right()
                            - pad
                            - block::measure(painter, &number, block::unit::MICRO)
                            - 8.0,
                        rect.bottom() - 9.0,
                    ),
                    egui::Vec2::splat(9.0),
                ),
                Weight::Hair,
                kind_ink,
            );
        }

        // A line down each gap between columns.
        //
        // The gap alone divided them on a black ground, where a column is
        // a plane against nothing. On paper the same gap is a narrow band
        // of near-white between two other near-whites, and a boundary the
        // eye has to look for is a boundary that is not doing its job.
        //
        // BETWEEN the columns and not around each one: an outline per
        // column would draw every seam twice and put a line down the
        // outside edges, where there is nothing to separate.
        let margin_bottom = field.max.y - margin;
        for slot in 1..shown.len() {
            let x = (Self::head_rect(field, slot).min.x - gap / 2.0).floor() + 0.5;
            painter.line_segment(
                [egui::pos2(x, top), egui::pos2(x, margin_bottom)],
                egui::Stroke::new(1.0, self.alphabet().edge.color),
            );
        }

        // The absorbed keystroke, in the session's own geometry: the
        // heads and every drawn scene row together, because the cursor
        // can be refused at the bottom of the lattice as well as at the
        // top of the strip. Every edge is drawn, because a swallowed key
        // is indistinguishable from a broken one.
        let drawn = shown.len() as f32;
        let rows = self.scene_window(field).len() as f32;
        let span = egui::Rect::from_min_size(
            egui::pos2(field.min.x + margin, top),
            egui::vec2(
                ADDRESS_W + drawn * TRACK_W + (drawn - 1.0) * gap,
                TRACK_H + section_gap() + rows * (scenes::SLOT_H + rows_gap),
            ),
        );
        let inset = gap.max(4.0);

        // Tracks the window is not showing. A SHORT tick, where a refusal
        // is a full-height rule: the two can never appear on the same edge
        // at the same time — a step toward hidden tracks scrolls instead of
        // refusing — but they are still different marks, because a
        // performer must not have to reason about which one they are
        // looking at.
        let elsewhere = TRACK_H / 3.0;
        let middle = top + TRACK_H / 2.0;
        if first > 0 {
            let mut shapes = Vec::new();
            circuit::annotation_arrow(
                &mut shapes,
                egui::pos2(span.left(), middle),
                egui::pos2(span.left() - inset - elsewhere * 0.5, middle),
                self.alphabet().ink.color,
            );
            for shape in shapes {
                painter.add(shape);
            }
        }
        if last < heads.len() {
            let mut shapes = Vec::new();
            circuit::annotation_arrow(
                &mut shapes,
                egui::pos2(span.right(), middle),
                egui::pos2(span.right() + inset + elsewhere * 0.5, middle),
                self.alphabet().ink.color,
            );
            for shape in shapes {
                painter.add(shape);
            }
        }
        match self.refusal.map(|refusal| refusal.reason) {
            Some(RefusalReason::Edge(step)) => {
                let (a, b) = match step {
                    Step::Up => (
                        egui::pos2(span.left(), span.top() - inset),
                        egui::pos2(span.right(), span.top() - inset),
                    ),
                    Step::Down => (
                        egui::pos2(span.left(), span.bottom() + inset),
                        egui::pos2(span.right(), span.bottom() + inset),
                    ),
                    Step::Left => (
                        egui::pos2(span.left() - inset, span.top()),
                        egui::pos2(span.left() - inset, span.bottom()),
                    ),
                    Step::Right => (
                        egui::pos2(span.right() + inset, span.top()),
                        egui::pos2(span.right() + inset, span.bottom()),
                    ),
                };
                painter.line_segment([a, b], egui::Stroke::new(2.0, self.refusal_ink()));
            }
            Some(RefusalReason::Shallower) => {
                painter.rect_stroke(
                    span.expand(inset),
                    0.0,
                    egui::Stroke::new(2.0, self.refusal_ink()),
                    egui::StrokeKind::Outside,
                );
            }
            _ => {}
        }
    }

    /// The master: one column, pinned right, in both contents.
    ///
    /// It carries a head like a track's — so the eye reads it as a column
    /// of the same kind — and beneath it whatever the lattice is showing:
    /// its own channel strip in the mixer, and in the session nothing,
    /// because the master holds no clips and a column of empty slots
    /// would invite firing one.
    fn draw_master(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        cursor_shade: egui::Color32,
        phase: Phase,
    ) {
        let alpha = self.alphabet();
        let pad = design::px(design::space::STEP);
        let margin = design::px(design::space::ROOM);
        let gap = row_gap();
        let kind_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        let head = Self::master_rect(field);
        let focused = self.session_address() == Some(Address::Master);
        let fill = if focused {
            cursor_shade
        } else {
            alpha.well.color
        };
        let (title_ink, kind_ink) = if focused {
            (alpha.ground.color, alpha.well.color)
        } else {
            (alpha.ink.color, alpha.ink.color)
        };
        let powered = if focused {
            alpha.ground.color
        } else {
            alpha.edge.color.gamma_multiply(1.25)
        };
        kit::cached(
            painter,
            egui::Id::new("stage-master-head"),
            head,
            (fill, alpha.ground.color, title_ink, powered),
            |out| {
                circuit::relic_frame(out, head, fill, Weight::Heavy, powered);
                out.push(egui::Shape::closed_line(
                    circuit::relic_points(head.shrink(4.0)),
                    egui::Stroke::new(Weight::Hair.px(), alpha.edge.color),
                ));
                let core = egui::pos2(head.left() + 19.0, head.center().y - 2.0);
                circuit::relic_node(out, core, 11.0, powered, title_ink);
                Sign::Master.paint(
                    out,
                    egui::Rect::from_center_size(core, egui::Vec2::splat(16.0)),
                    Weight::Hair,
                    title_ink,
                );
                circuit::trace(
                    out,
                    &[
                        egui::pos2(head.left() + 39.0, head.top() + 7.0),
                        egui::pos2(head.right() - 20.0, head.top() + 7.0),
                        egui::pos2(head.right() - 14.0, head.top() + 13.0),
                    ],
                    Weight::Hair,
                    powered,
                );
            },
        );
        block::paint(
            painter,
            egui::Id::new("stage-master-title"),
            egui::pos2(head.left() + 39.0, head.top() + pad),
            egui::Align2::LEFT_TOP,
            block::unit::MICRO,
            "MASTER",
            title_ink,
        );
        painter.text(
            egui::pos2(head.left() + 39.0, head.bottom() - pad),
            egui::Align2::LEFT_BOTTOM,
            "OUT",
            kind_font,
            kind_ink,
        );
        if focused {
            let target = if self.mixing {
                egui::Rect::from_min_max(
                    head.min,
                    egui::pos2(
                        head.right(),
                        field.bottom() - design::px(design::space::ROOM),
                    ),
                )
            } else {
                head
            };
            crate::ui::nav_cursor::claim(
                painter,
                "stage-master-cursor",
                target,
                if self.mixing {
                    crate::ui::nav_cursor::Kind::Column
                } else {
                    crate::ui::nav_cursor::Kind::Cell
                },
                crate::ui::nav_cursor::Layer::Surface,
                alpha.ink.color,
            );
        }

        // The seam is the master rail. Every shown track elbows into its
        // top shoulder; the scene rails already terminate against its
        // vertical run in `draw_board`.
        let seam = (head.min.x - column_gap() / 2.0).floor() + 0.5;
        let rail_top = head.top() - 7.0;
        let rail_bottom = field.bottom() - margin;
        let tracks = self.strip_window(field);
        let mut rail_shapes = Vec::new();
        // The spine: the one rail every track hangs off, so it is the
        // one rail on the board drawn heavy. The elbows that reach it
        // stay hairlines — they connect, the spine carries.
        circuit::rail_weighted(
            &mut rail_shapes,
            egui::pos2(seam, rail_top),
            egui::pos2(seam, rail_bottom),
            &[0.0, 1.0],
            Weight::Heavy,
            powered,
        );
        for (slot, _) in tracks.clone().enumerate() {
            let from = egui::pos2(bus_x(field, slot), head.top() - 1.0);
            let to = egui::pos2(seam, rail_top - slot as f32 * 1.5);
            let path = circuit::elbow(from, to);
            circuit::trace(
                &mut rail_shapes,
                &path,
                Weight::Hair,
                powered.gamma_multiply(0.72),
            );
            circuit::pad(&mut rail_shapes, from, circuit::PAD, powered, true);
        }
        if phase.rolling && self.playing.iter().any(Option::is_some) {
            circuit::dashes(
                &mut rail_shapes,
                &[egui::pos2(seam, rail_top), egui::pos2(seam, rail_bottom)],
                phase.dash(),
                Weight::Heavy,
                alpha.live_dim.color,
            );
        }
        for shape in rail_shapes {
            painter.add(shape);
        }

        if !self.mixing {
            return;
        }
        let strip = mixer::strip_beneath(head, field.max.y - margin, gap);
        mixer::draw(
            painter,
            strip,
            &mixer::Channel {
                gain: self.song.master,
                pan: 0.0,
                muted: false,
                soloed: false,
                // The master is always in the path: everything that
                // sounds arrives here, and there is nothing above it that
                // could silence it.
                audible: true,
                switches: false,
                level: self.meters.master().level,
                peak: self.meters.master().peak,
                sends: [None; crate::sequencing::ReturnTrack::MAX],
                send_letters: ['?'; crate::sequencing::ReturnTrack::MAX],
                is_return: false,
            },
            design::px(design::space::SNUG),
            alpha,
            0,
        );
    }

    /// The mixer: one channel strip per shown track, hanging beneath its
    /// own head.
    ///
    /// Focus is NOT marked here. The cursor addresses a channel as a
    /// whole while the mixer is up, so the head above the strip is the
    /// cell the cursor stands in, and `draw_tracks` has already inverted
    /// it. Marking the strip too would spend focus ink twice on one
    /// thing — and dimming the other strips, the other way to show which
    /// is current, would destroy the comparison between levels that is
    /// the entire reason a mixer puts them side by side.
    fn draw_mixer(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        _lattice: &FocusLattice,
        _cursor_shade: egui::Color32,
    ) {
        // The strip hangs under its head by the row gap, and keeps the
        // snug rhythm INSIDE itself: the lattice's spacing groups columns,
        // while a channel's own parts are spaced by what they are.
        let gap = row_gap();
        let inner = design::px(design::space::SNUG);
        let margin = design::px(design::space::ROOM);
        let tracks = self.strip_window(field);
        if tracks.is_empty() {
            return;
        }
        let bottom = field.max.y - margin;
        let channels = mixer::channels(&self.song, &self.meters.readings());
        let shown = tracks.len();
        for (slot, track) in tracks.enumerate() {
            let Some(channel) = channels.get(track) else {
                continue;
            };
            let head = Self::head_rect(field, slot);
            let strip = mixer::strip_beneath(head, bottom, gap);
            mixer::draw(painter, strip, channel, inner, self.alphabet(), track as u8);
        }
        // The returns stand NEXT TO THE MASTER, at the right-hand end of
        // the desk where the sends are going, rather than after the last
        // track where they would move every time a track is added. Laid
        // right to left from the master so the order reads TAPE, SHADOW,
        // MASTER however many tracks are shown.
        let master = Self::master_rect(field);
        let rails: Vec<mixer::Reading> = (0..self.song.console.aux.len())
            .map(|index| self.meters.rail(vitals::RETURN_METER + index))
            .collect();
        let returns = mixer::returns(&self.song, &rails);
        let column = TRACK_W + column_gap();
        let last_track = Self::head_rect(field, shown.saturating_sub(1));
        let mut placed: Vec<(usize, egui::Rect)> = Vec::new();
        for (index, ret) in returns.iter().enumerate() {
            let head = egui::Rect::from_min_size(
                egui::pos2(
                    master.left() - (returns.len() - index) as f32 * column,
                    master.top(),
                ),
                master.size(),
            );
            // A return with no column left is not drawn this frame,
            // rather than drawn over the tracks it would collide with.
            if head.left() < last_track.right() + gap {
                continue;
            }
            let variant = (8 + index) as u8;
            mixer::draw_return_head(painter, head, ret, self.alphabet(), variant);
            let strip = mixer::strip_beneath(head, bottom, gap);
            mixer::draw(
                painter,
                strip,
                &ret.channel,
                inner,
                self.alphabet(),
                variant,
            );
            placed.push((index, strip));
        }
        self.draw_send_loom(painter, field, bottom, gap, inner, &channels, &placed);
    }

    /// The send loom: the cable that makes a send a PATH rather than a
    /// number on a strip.
    ///
    /// Every channel's send rail is the same rail — the one that runs to
    /// that return — so the mixer stitches them together across the gaps
    /// between the strips and carries the line on to the return's own
    /// column. Each return keeps its colour, the same one the band's
    /// loom uses, so a send followed by eye in one view is the same
    /// send in the other. A segment leaving an open send is bright; one
    /// leaving a closed send is the ghost of where it could go.
    #[allow(clippy::too_many_arguments)]
    fn draw_send_loom(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        bottom: f32,
        gap: f32,
        inner: f32,
        channels: &[mixer::Channel],
        returns: &[(usize, egui::Rect)],
    ) {
        let tracks = self.strip_window(field);
        if tracks.is_empty() || returns.is_empty() {
            return;
        }
        let alpha = self.alphabet();
        let count = self
            .song
            .console
            .aux
            .len()
            .min(crate::sequencing::ReturnTrack::MAX);
        let strips: Vec<(usize, egui::Rect)> = tracks
            .clone()
            .enumerate()
            .map(|(slot, track)| {
                (
                    track,
                    mixer::strip_beneath(Self::head_rect(field, slot), bottom, gap),
                )
            })
            .collect();
        let mut shapes = Vec::new();
        for (slot, ret_strip) in returns {
            let Some(&ink) = RETURN_INK.get(*slot) else {
                continue;
            };
            let y = strips
                .first()
                .map(|(_, strip)| mixer::send_y(*strip, inner, count, true, *slot));
            let Some(y) = y else { continue };
            // Across every gap between the strips, and on to the
            // return's own column.
            let mut runs: Vec<(f32, f32, f32)> = Vec::new();
            for pair in strips.windows(2) {
                let ((left_track, left), (_, right)) = (pair[0], pair[1]);
                let open = channels
                    .get(left_track)
                    .and_then(|channel| channel.sends[*slot])
                    .unwrap_or(0.0);
                runs.push((left.right(), right.left(), open));
            }
            if let Some((last_track, last)) = strips.last() {
                let open = channels
                    .get(*last_track)
                    .and_then(|channel| channel.sends[*slot])
                    .unwrap_or(0.0);
                runs.push((last.right(), ret_strip.left(), open));
            }
            for (from, to, open) in runs {
                if to <= from {
                    continue;
                }
                circuit::trace(
                    &mut shapes,
                    &[egui::pos2(from, y), egui::pos2(to, y)],
                    Weight::Hair,
                    tint(ink, 0.22 + 0.78 * open.clamp(0.0, 1.0)),
                );
            }
            // Where each channel taps the line: a pad on its own mark,
            // filled when it is sending.
            for (track, strip) in &strips {
                let Some(open) = channels
                    .get(*track)
                    .and_then(|channel| channel.sends[*slot])
                else {
                    continue;
                };
                circuit::pad(
                    &mut shapes,
                    egui::pos2(mixer::send_x(*strip, inner, open), y),
                    circuit::PAD - 1.0,
                    tint(ink, 0.3 + 0.7 * open),
                    open > 0.005,
                );
            }
            // And where it lands: the return's own column, tapped on its
            // wall so the cable plainly arrives somewhere.
            circuit::pad(
                &mut shapes,
                egui::pos2(ret_strip.left(), y),
                circuit::PAD,
                ink,
                true,
            );
            circuit::trace(
                &mut shapes,
                &[
                    egui::pos2(ret_strip.left(), y),
                    egui::pos2(ret_strip.left() + 8.0, y),
                ],
                Weight::Heavy,
                ink,
            );
        }
        let _ = alpha;
        painter.extend(shapes);
    }

    /// The scene lattice: one slot per (shown track, scene), stacked under
    /// the heads. Faint by construction — a resting slot is a SURFACE
    /// plane on the GROUND, the same step in value a head is, and the
    /// dimmest mark the alphabet has. It holds nothing yet, so it says
    /// nothing louder than "a place exists here".
    fn draw_scenes(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        lattice: &FocusLattice,
        cursor_shade: egui::Color32,
        phase: Phase,
    ) {
        let gap = row_gap();
        let pad = design::px(design::space::STEP);
        let tracks = self.strip_window(field);
        let rows = self.scene_window(field);
        if tracks.is_empty() || rows.is_empty() {
            return;
        }

        // The focused slot inverts, exactly as a head does: one signal,
        // drawn one way, wherever the cursor stands on the session.
        let focused = lattice
            .cursor()
            .map(|cursor| Address::of(cursor, self.song.tracks.len()));
        let focused_scene = match focused {
            Some(Address::Slot { scene, .. }) => Some(scene),
            _ => None,
        };

        // The scene addresses, in the gutter: the row's number, lit when
        // the cursor is on that row and quiet otherwise. The heads get no
        // number here because their number is on them.
        let head = Self::head_rect(field, 0);
        for (line, scene) in rows.clone().enumerate() {
            let rect = scenes::slot_beneath(head, line, gap, section_gap());
            let alpha = self.alphabet();
            let ink = if focused_scene == Some(scene) {
                alpha.ink.color
            } else {
                alpha.ink.color.gamma_multiply(0.74)
            };
            let powered = if focused_scene == Some(scene) {
                alpha.ink.color
            } else {
                alpha.edge.color.gamma_multiply(1.1)
            };
            let node = egui::pos2(head.left() - 9.0, rect.center().y);
            // The gutter's mark is a graduation on the sheet's edge,
            // long for the row and short beside it, rather than a node.
            let mut shapes = Vec::new();
            for (reach, weight) in [(7.0f32, Weight::Heavy), (3.0, Weight::Hair)] {
                circuit::trace(
                    &mut shapes,
                    &[
                        egui::pos2(node.x - reach, node.y + if reach > 5.0 { 0.0 } else { 4.0 }),
                        egui::pos2(node.x + reach, node.y + if reach > 5.0 { 0.0 } else { 4.0 }),
                    ],
                    weight,
                    powered,
                );
            }
            circuit::trace(
                &mut shapes,
                &[
                    egui::pos2(head.left() - ADDRESS_W + 2.0, rect.center().y + 9.0),
                    egui::pos2(head.left() - 29.0, rect.center().y + 9.0),
                    egui::pos2(head.left() - 23.0, rect.center().y + 3.0),
                ],
                Weight::Hair,
                powered,
            );
            for shape in shapes {
                painter.add(shape);
            }
            let number = format!("{:02}", scene + 1);
            block::paint(
                painter,
                egui::Id::new(("stage-scene-address", scene)),
                egui::pos2(head.left() - ADDRESS_W + 1.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                block::unit::MICRO,
                &number,
                ink,
            );
            Sign::Register((scene % 16) as u8).painted(
                painter,
                egui::Id::new(("stage-scene-register", scene)),
                egui::Rect::from_center_size(node, egui::Vec2::splat(10.0)),
                Weight::Hair,
                ink,
            );
        }

        for (slot, track) in tracks.clone().enumerate() {
            let head = Self::head_rect(field, slot);
            for (line, scene) in rows.clone().enumerate() {
                let rect = scenes::slot_beneath(head, line, gap, section_gap());
                let here = focused == Some(Address::Slot { track, scene });
                let selected = self.session_selection.cells.contains(&(track, scene));
                let mark = scenes::mark(&self.song, track, scene);
                let alpha = self.alphabet();
                let fill = if here {
                    cursor_shade
                } else if selected {
                    alpha.ink.color.gamma_multiply(0.34)
                } else if mark.is_some() {
                    alpha.surface.color
                } else {
                    alpha.well.color
                };
                let figure_ink = if here {
                    alpha.ground.color
                } else {
                    alpha.ink.color
                };
                // Structure ink, not live ink. An empty address on the
                // session is an empty place on a ruled sheet; spending
                // the sounding hue on every one of them left nothing
                // for the clips that are actually playing. A SELECTED
                // address is the exception: the hand has named it, so it
                // is drawn in content ink like anything else the hand
                // has hold of.
                let cell_power = if here {
                    alpha.ground.color
                } else if selected {
                    alpha.ink.color
                } else if mark.is_some() {
                    alpha.edge.color.gamma_multiply(1.25)
                } else {
                    alpha.edge.color.gamma_multiply(0.8)
                };
                kit::cached(
                    painter,
                    egui::Id::new(("stage-scene-cell", track, scene)),
                    rect,
                    (fill, figure_ink, cell_power, mark.is_some(), selected),
                    |out| {
                        // AN EMPTY ADDRESS IS EMPTY. It gets the row's
                        // ruling and a registration tick at its left
                        // edge, and nothing else — no casing, no node.
                        // The sheet is ruled; the clips are what is
                        // written on it.
                        if mark.is_none() && !here && !selected {
                            circuit::trace(
                                out,
                                &[
                                    egui::pos2(rect.left(), rect.bottom() + 0.5),
                                    egui::pos2(rect.right(), rect.bottom() + 0.5),
                                ],
                                Weight::Hair,
                                cell_power.gamma_multiply(0.55),
                            );
                            circuit::trace(
                                out,
                                &[
                                    egui::pos2(rect.left() + 0.5, rect.bottom() + 0.5),
                                    egui::pos2(rect.left() + 0.5, rect.bottom() - 4.5),
                                ],
                                Weight::Hair,
                                cell_power,
                            );
                            return;
                        }
                        // A filled address is a square component on the
                        // sheet, the way a written entry is.
                        out.push(egui::Shape::rect_filled(rect, 0.0, fill));
                        circuit::trace(
                            out,
                            &[
                                rect.left_top(),
                                rect.right_top(),
                                rect.right_bottom(),
                                rect.left_bottom(),
                                rect.left_top(),
                            ],
                            Weight::Hair,
                            cell_power,
                        );
                        if mark.is_some() {
                            circuit::trace(
                                out,
                                &[
                                    egui::pos2(rect.left() + 8.0, rect.top() + 5.0),
                                    egui::pos2(rect.center().x, rect.top() + 5.0),
                                    egui::pos2(rect.center().x + 5.0, rect.top()),
                                ],
                                Weight::Hair,
                                cell_power,
                            );
                            for offset in [0.0, 5.0, 10.0] {
                                circuit::pad(
                                    out,
                                    egui::pos2(rect.right() - 20.0 + offset, rect.bottom() - 4.0),
                                    2.0,
                                    cell_power,
                                    offset == 10.0,
                                );
                            }
                        }
                    },
                );

                if here {
                    crate::ui::nav_cursor::claim(
                        painter,
                        ("stage-session-cell-cursor", track, scene),
                        rect,
                        crate::ui::nav_cursor::Kind::Cell,
                        crate::ui::nav_cursor::Layer::Surface,
                        alpha.ink.color,
                    );
                }

                if self.playing_on(track) == Some(scene) {
                    let mut shapes = Vec::new();
                    circuit::trace(
                        &mut shapes,
                        &[
                            rect.left_top(),
                            rect.right_top(),
                            rect.right_bottom(),
                            rect.left_bottom(),
                            rect.left_top(),
                        ],
                        Weight::Heavy,
                        alpha.live_dim.color,
                    );
                    if phase.rolling {
                        let mut path = circuit::relic_points(rect.shrink(1.5));
                        if let Some(first) = path.first().copied() {
                            path.push(first);
                        }
                        circuit::dashes(
                            &mut shapes,
                            &path,
                            phase.dash(),
                            Weight::Heavy,
                            alpha.live.color,
                        );
                    }
                    let live = motion::pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
                    shapes.push(egui::Shape::rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(rect.left(), rect.top() + 6.0),
                            egui::vec2(POINT, rect.height() - 12.0),
                        ),
                        0.0,
                        live,
                    ));
                    for shape in shapes {
                        painter.add(shape);
                    }
                }

                let Some(mark) = mark else {
                    continue;
                };
                Sign::Register((scene % 16) as u8).painted(
                    painter,
                    egui::Id::new(("stage-cell-register", track, scene)),
                    egui::Rect::from_center_size(
                        egui::pos2(rect.left() + pad + 3.0, rect.center().y),
                        egui::Vec2::splat(14.0),
                    ),
                    Weight::Hair,
                    figure_ink,
                );
                block::paint(
                    painter,
                    egui::Id::new(("stage-cell-number", track, scene)),
                    egui::pos2(rect.right() - pad, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    block::unit::MICRO,
                    &mark.label,
                    figure_ink,
                );
            }
        }

        // Scenes the window is not showing, marked the way hidden tracks
        // are: a short tick past the edge they lie beyond.
        let first_slot = scenes::slot_beneath(Self::head_rect(field, 0), 0, gap, section_gap());
        let last_slot = scenes::slot_beneath(
            Self::head_rect(field, 0),
            rows.len() - 1,
            gap,
            section_gap(),
        );
        let elsewhere = TRACK_W / 3.0;
        let middle = first_slot.center().x;
        let inset = gap.max(4.0);
        if rows.start > 0 {
            let mut shapes = Vec::new();
            circuit::annotation_arrow(
                &mut shapes,
                egui::pos2(middle, first_slot.top()),
                egui::pos2(middle, first_slot.top() - inset - elsewhere * 0.35),
                self.alphabet().ink.color,
            );
            for shape in shapes {
                painter.add(shape);
            }
        }
        if rows.end < self.song.session.scenes.len() {
            let mut shapes = Vec::new();
            circuit::annotation_arrow(
                &mut shapes,
                egui::pos2(middle, last_slot.bottom()),
                egui::pos2(middle, last_slot.bottom() + inset + elsewhere * 0.35),
                self.alphabet().ink.color,
            );
            for shape in shapes {
                painter.add(shape);
            }
        }
    }

    /// The ancestry zone: one miniature grid per level above the active
    /// one, root first, each with its entered square lit. It lives at the
    /// left end of the vitals strip — a constant home the eye learns once;
    /// at the root the strip is simply empty, which itself reads as "top
    /// level, all quiet".
    /// The stream screen: a dark pane set into the vitals strip that
    /// states the stream's facts in figures rather than words — the
    /// codex's numerals for the rate, a row of bits for the buffer, a
    /// gauge for the latency, pads for the channels, and the backend's
    /// seal. Read once it is learned, and legible at a glance as "the
    /// deck's own instruments" before then. It is a SCREEN, so it is the
    /// one recess on a strip that is otherwise all surface, and it
    /// carries the strip's chamfer so it reads as fitted. Everything in
    /// it is drawn at a scale taken from the pane's height, so the pane
    /// can be made larger or smaller and its figures keep their places.
    fn draw_stream(&self, painter: &egui::Painter, screen: egui::Rect, phase: Phase) {
        if screen.width() < 80.0 || screen.height() < 16.0 {
            return;
        }
        // One unit of the pane: its height over the height the figures
        // were drawn for.
        let k = screen.height() / 30.0;
        let alpha = self.alphabet();
        let plate = shell_plate(self.polarity);
        let stream = self.vitals.stream().copied();
        let running = self.vitals.running();
        let facts = stream.map(|s| {
            (
                s.sample_rate,
                s.buffer_frames,
                s.latency_frames,
                s.inputs,
                s.outputs,
                s.backend,
            )
        });
        kit::cached(
            painter,
            egui::Id::new("stage-stream-screen"),
            screen,
            (
                alpha.ground.color,
                plate,
                alpha.edge.color,
                alpha.ink.color,
                facts,
            ),
            |out| {
                circuit::panel_variant(
                    out,
                    screen,
                    Some(alpha.ground.color),
                    plate,
                    Some((Weight::Hair, alpha.edge.color)),
                    2,
                );
                circuit::panel_frame_variant(
                    out,
                    screen.shrink(3.0 * k),
                    Weight::Hair,
                    alpha.edge.color.gamma_multiply(0.48),
                    0,
                );
                let inner = screen.shrink2(egui::vec2(10.0 * k, 5.0 * k));
                let y = inner.center().y;
                let mut x = inner.left();
                // The engine's own sign opens the line.
                Sign::Engine.paint(
                    out,
                    egui::Rect::from_center_size(
                        egui::pos2(x + 7.0 * k, y),
                        egui::Vec2::splat(14.0 * k),
                    ),
                    Weight::Hair,
                    alpha.ink.color,
                );
                x += 22.0 * k;
                let Some(stream) = stream else {
                    // No stream: the screen is lit and empty, one hollow
                    // pad where the facts would begin.
                    circuit::pad(
                        out,
                        egui::pos2(x + 4.0 * k, y),
                        circuit::PAD * k,
                        alpha.edge.color,
                        false,
                    );
                    return;
                };
                // The rate, in the codex's numerals, as kilohertz: two or
                // three figures, no unit — the unit is the position.
                let khz = (stream.sample_rate / 1000).min(999);
                let digits: Vec<u8> = khz.to_string().bytes().map(|b| b - b'0').collect();
                for digit in digits {
                    Sign::Numeral(digit).paint(
                        out,
                        egui::Rect::from_center_size(
                            egui::pos2(x + 5.0 * k, y),
                            egui::Vec2::splat(11.0 * k),
                        ),
                        Weight::Hair,
                        alpha.ink.color,
                    );
                    x += 12.0 * k;
                }
                x += 6.0 * k;
                circuit::via(out, egui::pos2(x, y), alpha.edge.color, alpha.ground.color);
                x += 8.0 * k;
                // The buffer, in binary: twelve bits, which reach 4096
                // frames, every value the device could plausibly open.
                let unit = 3.0 * k;
                circuit::binary(
                    out,
                    egui::pos2(x, y - unit * 0.5),
                    unit,
                    stream.buffer_frames.min(4095),
                    12,
                    alpha.ink.color,
                );
                x += 12.0 * (unit + 1.0) + 6.0 * k;
                circuit::via(out, egui::pos2(x, y), alpha.edge.color, alpha.ground.color);
                x += 8.0 * k;
                // The latency, as a gauge over forty milliseconds: unlit
                // when the device will not say.
                let gauge = egui::Rect::from_min_size(
                    egui::pos2(x, y - 3.0 * k),
                    egui::vec2(40.0 * k, 6.0 * k),
                );
                let lit = stream
                    .latency_ms()
                    .map_or(0.0, |ms| (ms / 40.0).clamp(0.0, 1.0));
                circuit::tick_bar(out, gauge, 8, lit, alpha.ink.color, alpha.edge.color, true);
                x += 46.0 * k;
                circuit::via(out, egui::pos2(x, y), alpha.edge.color, alpha.ground.color);
                x += 8.0 * k;
                // The channels: a row of hollow pads for what comes in
                // above a row of filled pads for what goes out.
                for (row, (count, filled)) in [(stream.inputs, false), (stream.outputs, true)]
                    .into_iter()
                    .enumerate()
                {
                    let py = y - 3.0 * k + row as f32 * 6.0 * k;
                    for n in 0..count.min(8) {
                        circuit::pad(
                            out,
                            egui::pos2(x + 2.0 * k + n as f32 * 5.0 * k, py),
                            3.0 * k,
                            alpha.ink.color,
                            filled,
                        );
                    }
                }
                x += 8.0 * 5.0 * k + 4.0 * k;
                // The backend's seal: a barcode cut from its name, so one
                // backend always wears one mark.
                let seal = egui::Rect::from_min_max(
                    egui::pos2(x, y - 6.0 * k),
                    egui::pos2(inner.right().max(x + 24.0 * k), y + 6.0 * k),
                );
                let mut rng = kit::Rng::seeded(("stream-seal", stream.backend));
                circuit::barcode(out, seal, &mut rng, alpha.edge.color);
            },
        );
        // The trace along the screen's foot: current, while the engine
        // runs and the song rolls; a still hairline otherwise. The one
        // moving thing on the strip, and it moves because the deck is.
        let foot = [
            egui::pos2(screen.left() + 8.0 * k, screen.bottom() - 3.0 * k),
            egui::pos2(screen.right() - 8.0 * k, screen.bottom() - 3.0 * k),
        ];
        let mut marks = Vec::new();
        if running && phase.rolling {
            circuit::dashes(
                &mut marks,
                &foot,
                phase.dash(),
                Weight::Hair,
                alpha.live_dim.color,
            );
        } else {
            circuit::trace(&mut marks, &foot, Weight::Hair, alpha.edge.color);
        }
        painter.extend(marks);
        if self.polarity == design::Polarity::Dark {
            crate::shell::screen::register(
                painter,
                screen.shrink2(egui::vec2(5.0 * k, 4.0 * k)),
                crate::shell::screen::State::new(
                    if phase.rolling { phase.beat } else { 0.0 },
                    if running {
                        0.18 + phase.pulse() * 0.72
                    } else {
                        0.0
                    },
                ),
            );
        }
    }

    fn draw_breadcrumb(&self, painter: &egui::Painter, zone: egui::Rect) {
        const MINI_CELL: f32 = 5.0;
        const MINI_GAP: f32 = 1.0;
        const MARGIN: f32 = 16.0;
        const SPACING: f32 = 12.0;

        let levels = self.focus.levels();
        let ancestors = &levels[..levels.len() - 1];
        // Every shape draws through its miniature, so an ancestor the eye
        // must account for is never silently skipped — a level that drew
        // nothing would report "top level, all quiet" from inside a
        // descent, which is a false ancestry rather than a missing mark.
        let tallest = ancestors
            .iter()
            .map(|scope| scope.miniature().rows)
            .max()
            .unwrap_or(0);
        if tallest == 0 {
            return;
        }

        let mini_h = tallest as f32 * (MINI_CELL + MINI_GAP) - MINI_GAP;
        let top = (zone.center().y - mini_h / 2.0).floor();
        let mut corner = egui::pos2(zone.min.x + MARGIN, top);
        for scope in ancestors {
            let level = scope.miniature();
            if level.cols == 0 || level.rows == 0 {
                continue;
            }
            let span = egui::vec2(
                level.cols as f32 * (MINI_CELL + MINI_GAP) - MINI_GAP,
                level.rows as f32 * (MINI_CELL + MINI_GAP) - MINI_GAP,
            );
            // Shorter shapes sit centred against the tallest, so the strip
            // reads as one row of levels rather than a ragged top edge.
            corner.y = (top + (mini_h - span.y) / 2.0).floor();
            for row in 0..level.rows {
                for col in 0..level.cols {
                    let rect = egui::Rect::from_min_size(
                        corner
                            + egui::vec2(
                                col as f32 * (MINI_CELL + MINI_GAP),
                                row as f32 * (MINI_CELL + MINI_GAP),
                            ),
                        egui::vec2(MINI_CELL, MINI_CELL),
                    );
                    let shade = if (col, row) == (level.col, level.row) {
                        self.focused()
                    } else {
                        self.square()
                    };
                    painter.rect_filled(rect, 0.0, shade);
                }
            }
            corner.x += span.x + SPACING;
        }
    }

    /// The time end of the vitals strip. It reports but never acts: the
    /// keyboard moves time, and focus remains in the sovereign field.
    /// The engine's own state, on the periphery, as one mark.
    ///
    /// Only drawn when there is NO engine. A running one is the ordinary
    /// case and needs no announcement — a light that is always on says
    /// nothing — while silence with nothing behind the stage is a
    /// different problem from silence with a full graph, and a surface
    /// that draws them identically makes the reader guess which they have.
    ///
    /// It is JEOPARDY rather than ink: nothing this frame does will make
    /// a sound, and that is a thing at stake rather than a fact to read.
    fn draw_engine(&self, painter: &egui::Painter, zone: egui::Rect) {
        let margin = design::px(design::space::ROOM);
        let gap = design::px(design::space::ROOM);
        let font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        let alpha = self.alphabet();
        let mut right = zone.max.x - margin;

        // The song's name, and whether it is safe. Structure ink: a fact
        // to read when looked for, never a thing that competes with the
        // message beside it.
        let title = self
            .path
            .as_deref()
            .map(document::title)
            .unwrap_or_else(|| "untitled".to_owned());
        let title = if self.dirty {
            format!("{title} *")
        } else {
            title
        };
        let title_rect = painter.text(
            egui::pos2(right, zone.center().y),
            egui::Align2::RIGHT_CENTER,
            title,
            font.clone(),
            alpha.edge.color,
        );
        right = title_rect.min.x - gap;

        // The engine, in as many words as it needs and no more: a
        // running engine says nothing, a dropped block says so loudly
        // for as long as it takes to be seen, and the count stays behind
        // quietly for whoever looks later.
        let Some((words, tone)) = self.vitals.words() else {
            return;
        };
        let ink = match tone {
            vitals::Tone::Alarm => alpha.jeopardy_active.color,
            vitals::Tone::Quiet => alpha.edge.color,
        };
        painter.text(
            egui::pos2(right, zone.center().y),
            egui::Align2::RIGHT_CENTER,
            words,
            font,
            ink,
        );
    }

    fn draw_transport(&self, painter: &egui::Painter, zone: egui::Rect) {
        let place = self.transport.place(&self.song);
        let readout = place.readout();
        let beats = beat_cells(place);
        let tempo = format!(
            "{:.0}",
            self.song
                .bpm_at(self.transport.tick(), transport::DEFAULT_BPM)
        );

        let body = egui::FontId::monospace(design::px(design::type_scale::BODY));
        let margin = design::px(design::space::ROOM);
        let gap = design::px(design::space::ROOM);
        let center_y = zone.center().y;
        let mut right = zone.max.x - margin;

        // The clock's face: an actual meter wheel at the strip's shoulder.
        // One angular pad per score beat, the current beat filled, and the
        // denominator cut into the hub. It looks ceremonial because the
        // song's meter is radial here, not because fake runes were added.
        {
            let r = (zone.height() * 0.36).min(22.0);
            let c = egui::pos2(zone.min.x + margin + r, center_y);
            let alpha = self.alphabet();
            let ink_col = alpha.ink.color;
            let ground = alpha.ground.color;
            let current_ink = if self.transport.motion().is_rolling() {
                alpha.live.color
            } else {
                ink_col
            };
            let denominator: Vec<u8> = place
                .denominator
                .to_string()
                .bytes()
                .map(|byte| byte - b'0')
                .collect();
            let face = egui::Rect::from_center_size(c, egui::Vec2::splat(r * 2.4));
            kit::cached(
                painter,
                egui::Id::new("stage-clock"),
                face,
                (
                    ink_col,
                    current_ink,
                    ground,
                    place.beat,
                    place.beats_per_bar,
                    place.denominator,
                ),
                |out| {
                    out.push(egui::Shape::circle_stroke(
                        c,
                        r,
                        egui::Stroke::new(Weight::Heavy.px(), ink_col),
                    ));
                    out.push(egui::Shape::circle_stroke(
                        c,
                        r - 5.0,
                        egui::Stroke::new(Weight::Hair.px(), alpha.edge.color),
                    ));
                    let beats = place.beats_per_bar.max(1);
                    for beat in 1..=beats {
                        let angle = -std::f32::consts::FRAC_PI_2
                            + std::f32::consts::TAU * (beat - 1) as f32 / beats as f32;
                        let at = c + egui::vec2(angle.cos(), angle.sin()) * (r - 2.5);
                        let current = beat == place.beat.min(beats);
                        circuit::pad(
                            out,
                            at,
                            if current {
                                circuit::PAD + 1.0
                            } else {
                                circuit::PAD - 1.0
                            },
                            if current {
                                current_ink
                            } else {
                                alpha.edge.color
                            },
                            current,
                        );
                    }
                    let digit_w = 8.0;
                    let start = c.x - denominator.len() as f32 * digit_w * 0.5;
                    for (index, digit) in denominator.iter().copied().enumerate() {
                        Sign::Numeral(digit).paint(
                            out,
                            egui::Rect::from_center_size(
                                egui::pos2(start + digit_w * (index as f32 + 0.5), c.y),
                                egui::Vec2::splat(8.0),
                            ),
                            Weight::Hair,
                            ink_col,
                        );
                    }
                    circuit::brackets(out, face.shrink(2.0), 5.0, Weight::Hair, alpha.edge.color);
                },
            );
        }

        let tempo_rect = block::paint(
            painter,
            egui::Id::new("stage-tempo"),
            egui::pos2(right, center_y),
            egui::Align2::RIGHT_CENTER,
            block::unit::MICRO,
            &tempo,
            self.alphabet().ink.color,
        );
        right = tempo_rect.min.x - gap;

        let beat_color = if self.transport.motion().is_rolling() {
            self.alphabet().live.color
        } else {
            self.alphabet().ink.color
        };
        let beat_rect = painter.text(
            egui::pos2(right, center_y),
            egui::Align2::RIGHT_CENTER,
            beats,
            body.clone(),
            beat_color,
        );
        right = beat_rect.min.x - gap;

        let readout_rect = block::paint(
            painter,
            egui::Id::new("stage-readout"),
            egui::pos2(right, center_y),
            egui::Align2::RIGHT_CENTER,
            block::unit::TITLE,
            &readout,
            self.alphabet().ink.color,
        );

        let mut left = readout_rect.min.x - gap;
        // What the clock runs: the session's scene, or the song. Said
        // only for the song, which is the departure from the norm.
        if self.transport.mode() == transport::Mode::Song {
            let word = block::paint(
                painter,
                egui::Id::new("stage-transport-mode"),
                egui::pos2(left, center_y),
                egui::Align2::RIGHT_CENTER,
                block::unit::MICRO,
                transport::Mode::Song.word(),
                if self.transport.motion().is_rolling() {
                    self.alphabet().live.color
                } else {
                    self.alphabet().ink.color
                },
            );
            left = word.min.x - gap;
        }
        // The arrangement armed: ARM while it waits, REC while the
        // session plays into it.
        if self.arming {
            let word = block::paint(
                painter,
                egui::Id::new("stage-arming"),
                egui::pos2(left, center_y),
                egui::Align2::RIGHT_CENTER,
                block::unit::MICRO,
                if self.recording_song() { "REC" } else { "ARM" },
                self.alphabet().jeopardy_active.color,
            );
            left = word.min.x - gap;
        }
        if self.transport.motion() == Motion::Recording {
            block::paint(
                painter,
                egui::Id::new("stage-recording"),
                egui::pos2(left, center_y),
                egui::Align2::RIGHT_CENTER,
                block::unit::MICRO,
                "REC",
                self.alphabet().jeopardy_active.color,
            );
        }
        let phase = Phase::of(
            self.transport.motion().is_rolling(),
            self.transport.beat_phase(),
        );
        if self.polarity == design::Polarity::Dark {
            crate::shell::screen::register(
                painter,
                zone.shrink2(egui::vec2(8.0, 11.0)),
                crate::shell::screen::State::new(
                    if phase.rolling { phase.beat } else { 0.0 },
                    if phase.rolling {
                        0.24 + phase.pulse() * 0.70
                    } else {
                        0.0
                    },
                ),
            );
        }
    }

    /// The message zone: the frame's refusal named in words, in the same
    /// gray as the geometric marks and gone the same frame they are. Empty
    /// is the normal state — this strip earns ink only when something was
    /// declined (and later: confirmed, landed, or failed).
    /// The browser: a window that opens ABOVE the work rather than
    /// alongside it.
    ///
    /// Its own opaque ground, so what is beneath is hidden rather than
    /// shining through, and one edge to say where it ends. Nothing under
    /// it moves — it covers the corner it covers and then gives it back.
    ///
    fn draw_browser(&self, painter: &egui::Painter, zone: egui::Rect) {
        let Some(browser) = &self.browser else {
            return;
        };
        // The archive is one made object rather than a rectangular veil.
        // Its fill and its outline are cut from the same path, so every
        // recess exposes the field beneath instead of leaving a square
        // patch behind the decorative border.
        let alpha = self.alphabet();
        let shell = zone.shrink(2.0);
        kit::cached(
            painter,
            egui::Id::new("stage-browser-shell"),
            shell,
            (alpha.well.color, alpha.ground.color, alpha.ink.color),
            |out| {
                circuit::panel_variant(
                    out,
                    shell,
                    Some(alpha.well.color),
                    alpha.ground.color,
                    Some((Weight::Heavy, alpha.ink.color)),
                    1,
                );
                circuit::panel_frame_variant(
                    out,
                    shell.shrink(5.0),
                    Weight::Hair,
                    alpha.edge.color,
                    3,
                );
                circuit::corner_pads(out, shell.shrink(2.0), alpha.edge.color);
            },
        );

        let line = design::px(design::type_scale::BODY);
        let font = egui::FontId::monospace(line);
        let inset = design::px(design::space::ROOM);
        let measure =
            painter.layout_no_wrap("M".to_owned(), font.clone(), self.alphabet().ink.color);
        let cell = measure.size();
        let columns = ((zone.width() - inset * 2.0) / cell.x).floor() as usize;
        let rows = ((zone.height() - inset * 2.0) / cell.y).floor() as usize;
        if columns < 8 || rows < 5 {
            return;
        }

        let inner = columns - 2;
        let origin = egui::pos2(zone.min.x + inset, zone.min.y + inset);
        let at = |column: usize, row: usize| {
            origin + egui::vec2(column as f32 * cell.x, row as f32 * cell.y)
        };
        let text = |position: egui::Pos2, words: String, color: egui::Color32| {
            painter.text(position, egui::Align2::LEFT_TOP, words, font.clone(), color);
        };

        let bottom = rows - 1;

        // The pane's rules are DRAWN, not typed. Ruling characters are one
        // glyph per cell, so a border made of them is a row of separate
        // marks with a seam at every cell boundary and a baseline that is
        // not the cell's centre — at this size that reads as hatching
        // rather than as a line. `ui::glyph` already made this argument
        // for the family marks; the same reasoning ends at the same place.
        // A segment is one stroke, pixel-aligned, the same on every
        // machine, and independent of what the font happens to carry.
        let half = egui::vec2(cell.x / 2.0, cell.y / 2.0);
        let snap = |point: egui::Pos2| egui::pos2(point.x.round(), point.y.round());
        let frame =
            egui::Rect::from_min_max(snap(at(0, 0) + half), snap(at(columns - 1, bottom) + half));
        // What is being typed sits in a WELL: a step down from the plane
        // it is cut into, which divides it from the list without a rule
        // between them. A recess also says what the band is for — you
        // write into a surface, not onto one.
        let divider = snap(at(0, 2) + half).y;
        let search = egui::Rect::from_min_max(
            egui::pos2(frame.left(), frame.top()),
            egui::pos2(frame.right(), divider),
        );
        kit::cached(
            painter,
            egui::Id::new("stage-browser-search"),
            search,
            (alpha.ground.color, alpha.well.color, alpha.edge.color),
            |out| {
                circuit::panel_variant(
                    out,
                    search,
                    Some(alpha.ground.color),
                    alpha.well.color,
                    Some((Weight::Hair, alpha.edge.color)),
                    2,
                );
                circuit::pad(
                    out,
                    egui::pos2(search.right() - 10.0, search.center().y),
                    circuit::PAD,
                    alpha.edge.color,
                    true,
                );
            },
        );

        // Archive seal and the vertical rail-name make this overlay read
        // as a place, not merely as a list that happened to cover a pane.
        let archive_mark = egui::Rect::from_center_size(
            egui::pos2(frame.left() + 12.0, frame.top() + 9.0),
            egui::Vec2::splat(14.0),
        );
        kit::cached(
            painter,
            egui::Id::new("stage-browser-archive-mark"),
            archive_mark,
            alpha.edge.color,
            |out| Sign::Archive.paint(out, archive_mark, Weight::Hair, alpha.edge.color),
        );
        let rail_rect = egui::Rect::from_min_max(
            egui::pos2(frame.right() - 13.0, frame.bottom() - 78.0),
            egui::pos2(frame.right() - 3.0, frame.bottom() - 4.0),
        );
        // Drawn straight rather than through the mesh cache: a word on
        // the atlas must not be frozen into a mesh the atlas can outgrow.
        block::paint_vertical(
            painter,
            egui::pos2(rail_rect.left(), rail_rect.bottom()),
            1.0,
            "ARCHIVE",
            alpha.edge.color,
        );

        // The surface's ONE mute flourish, and the only mark in this pane
        // carrying neither command nor state — the precedent is the
        // transport's phase marks. It is the luminance ladder the pane is
        // drawn from, signed into the bottom rule: three rungs ascending.
        //
        // Two are missing, and their absence is the whole of it. GROUND is
        // the page it would be drawn on, and FOCUS is spent on the cursor,
        // which a signature is not allowed to borrow. A flourish that
        // shouted would be claiming importance it does not have.
        //
        // Recorded as a flourish because the charter permits exactly one
        // per surface and forbids a second: if another is ever wanted
        // here, this is the one that has to go.
        let alpha = self.alphabet();
        let rungs = [alpha.surface, alpha.edge, alpha.ink];
        let pitch = design::px(design::space::SNUG);
        let rise = design::px(design::space::HAIR);
        let mut signature = egui::pos2(frame.right() - pitch * rungs.len() as f32, frame.bottom());
        for rung in rungs {
            painter.line_segment(
                [signature, egui::pos2(signature.x, signature.y - rise)],
                egui::Stroke::new(1.0, rung.color),
            );
            signature.x += pitch;
        }

        // The yield of what has been typed, in the row where it is being
        // typed. A keystroke earns its place by removing uncertainty, and
        // this is the only place the reader can see whether the last one
        // did — the moment the number stops falling, arrowing is cheaper
        // than typing.
        let yield_mark = if browser.query().is_empty() {
            String::new()
        } else {
            browser.surviving_leaves().to_string()
        };
        let typed = fit_cells(
            &format!("{} {}", browser::glyph::PROMPT, browser.query()),
            inner.saturating_sub(yield_mark.chars().count() + 1),
        );
        text(at(1, 1), typed, self.alphabet().ink.color);
        if !yield_mark.is_empty() {
            let column = columns - 1 - yield_mark.chars().count();
            text(at(column, 1), yield_mark, self.alphabet().edge.color);
        }

        // One drawable line per visible row, plus a NOTE under any open
        // shelf that has nothing to show. The note sits where the missing
        // rows would be, because a sign beside the thing it describes is
        // stronger than a status message detached from it.
        enum Line<'a> {
            Row(usize, &'a browser::Row, &'a Node),
            Note(usize, String),
        }

        let tree = browser.rows();
        let mut lines = Vec::new();
        for (index, row) in tree.iter().enumerate() {
            let Some(node) = browser.node_at(&row.path) else {
                continue;
            };
            lines.push(Line::Row(index, row, node));
            let EntryKind::Shelf(shelf) = node.kind else {
                continue;
            };
            if !node.expanded || !node.children.is_empty() {
                continue;
            }
            lines.push(Line::Note(
                row.depth + 1,
                match browser.status_of(shelf) {
                    BrowserStatus::Scanning => "Scanning".to_owned(),
                    BrowserStatus::Unavailable => "No source".to_owned(),
                    BrowserStatus::Ready => "Empty".to_owned(),
                },
            ));
        }
        if tree.is_empty() {
            lines.push(Line::Note(
                0,
                if browser.query().is_empty() {
                    "Empty".to_owned()
                } else {
                    "No match".to_owned()
                },
            ));
        }

        let visible = rows - 4;
        // Scroll by the LINE the cursor is on, so a note never pushes the
        // addressed row off the bottom.
        let addressed = browser.cursor().and_then(|cursor| {
            lines
                .iter()
                .position(|line| matches!(line, Line::Row(index, _, _) if *index == cursor))
        });
        let start = addressed
            .unwrap_or(0)
            .saturating_sub(visible / 2)
            .min(lines.len().saturating_sub(visible));

        for slot in 0..visible {
            let screen_row = 3 + slot;
            let Some(line) = lines.get(start + slot) else {
                continue;
            };
            match line {
                Line::Row(index, row, node) => {
                    let addressed = Some(*index) == browser.cursor();
                    if addressed {
                        let cursor = egui::Rect::from_min_size(
                            at(1, screen_row),
                            egui::vec2(inner as f32 * cell.x, cell.y),
                        );
                        let mut shapes = Vec::new();
                        circuit::panel_variant(
                            &mut shapes,
                            cursor,
                            Some(self.focused()),
                            alpha.well.color,
                            None,
                            (*index % 4) as u8,
                        );
                        painter.extend(shapes);
                        if self.browser.is_some() {
                            crate::ui::nav_cursor::claim(
                                painter,
                                ("stage-browser-cursor", *index),
                                cursor,
                                crate::ui::nav_cursor::Kind::Row,
                                crate::ui::nav_cursor::Layer::Overlay,
                                alpha.focus.color,
                            );
                        }
                    }

                    // Depth is drawn, not implied: two cells per level, so
                    // the eye finds a heading's children by their left
                    // edge before reading a word of them.
                    let indent = row.depth * 2;

                    // The gate: two hairline segments, and whether it is
                    // open is said by WHERE the second one sits — across
                    // the middle while closed, dropped to the foot once
                    // open. The shape is the second frame's, which solved
                    // this before: an arrow glyph is a whole character of
                    // ink for one bit, and a column of them reads as a
                    // second margin competing with the labels.
                    //
                    // Copied rather than shared. `ui::kit` is the natural
                    // home for it, but every helper there takes a `Theme`
                    // and this frame carries none — and the frame it came
                    // from is the one being replaced. If the two ever have
                    // to agree, the SHAPE is the thing to lift.
                    let structure = if addressed {
                        self.alphabet().surface.color
                    } else {
                        self.alphabet().edge.color
                    };
                    let content = if addressed {
                        self.alphabet().ground.color
                    } else {
                        self.alphabet().ink.color
                    };
                    if node.is_branch() {
                        let arm = design::px(design::space::HAIR);
                        let hinge =
                            at(1 + indent, screen_row) + egui::vec2(cell.x / 2.0, cell.y / 2.0);
                        let ink = egui::Stroke::new(1.0, structure);
                        let back = (arm * 0.75).round();
                        painter.line_segment(
                            [
                                hinge + egui::vec2(-back, -arm),
                                hinge + egui::vec2(-back, arm),
                            ],
                            ink,
                        );
                        let foot = if node.expanded { arm } else { 0.0 };
                        painter.line_segment(
                            [
                                hinge + egui::vec2(-back, foot),
                                hinge + egui::vec2(arm, foot),
                            ],
                            ink,
                        );
                    }

                    // A CLOSED branch says how much is behind it. Opening a
                    // heading to find one device is a keystroke that bought
                    // nothing, and the count is the only way to know before
                    // spending it. Open branches drop it: the rows beneath
                    // are the answer, and a mark that repeats what is
                    // already on screen is ornament.
                    // Zero is a count, not a gap. Suppressing it would
                    // make ABSENCE carry the meaning "empty", which reads
                    // identically to a mark that failed to draw — and an
                    // empty shelf is worth exactly the keystroke it saves
                    // by saying so.
                    let count = if node.is_branch() && !node.expanded {
                        node.leaves().to_string()
                    } else {
                        String::new()
                    };
                    if !count.is_empty() {
                        let column = columns - 1 - count.chars().count();
                        text(at(column, screen_row), count.clone(), structure);
                    }

                    // The family's mark, in a cell reserved on EVERY row
                    // whether or not that row has one. A column that
                    // appeared and vanished would move the labels beside
                    // it, and geometry that shifts with content is
                    // geometry the eye has to re-learn each frame.
                    //
                    // Drawn at the structure rung: an esoteric mark
                    // whispers. One that shouted would be claiming an
                    // importance the heading does not have.
                    if let Some(mark) = node.mark {
                        let box_ = egui::Rect::from_min_size(
                            at(1 + indent + 1, screen_row),
                            egui::vec2(cell.x, cell.y),
                        );
                        glyph::paint(painter, box_.shrink(1.0), mark, structure);
                    }

                    // The label, character by character, so the ones the
                    // filter actually consumed can be told from the ones
                    // it merely passed over. With nothing typed every
                    // character is unmatched and the row is drawn flat, so
                    // this costs nothing until it says something.
                    let start = 1 + indent + 3;
                    let leaf_tail = usize::from(!node.is_branch()) * 3;
                    let room = (columns - 1 - leaf_tail)
                        .saturating_sub(start)
                        .saturating_sub(if count.is_empty() {
                            0
                        } else {
                            count.chars().count() + 1
                        });
                    let marks = browser::match_positions(&node.label, browser.query());
                    for (offset, letter) in node.label.chars().take(room).enumerate() {
                        let lit = marks.get(offset).copied().unwrap_or(false);
                        let ink = if lit && !addressed {
                            self.alphabet().focus.color
                        } else if lit {
                            self.alphabet().ground.color
                        } else if addressed {
                            self.alphabet().surface.color
                        } else {
                            content
                        };
                        text(at(start + offset, screen_row), letter.to_string(), ink);
                    }
                    if !node.is_branch() {
                        let y = at(columns - 3, screen_row).y + cell.y * 0.5;
                        let a = egui::pos2(at(columns - 3, screen_row).x, y);
                        let b = egui::pos2(at(columns - 1, screen_row).x, y);
                        let mut shapes = Vec::new();
                        circuit::trace(&mut shapes, &[a, b], Weight::Hair, structure);
                        circuit::pad(&mut shapes, b, circuit::PAD - 1.0, structure, addressed);
                        painter.extend(shapes);
                    }
                }
                // A note is never addressable, so it never inverts, and it
                // speaks a rung quieter than the rows it stands among.
                Line::Note(depth, words) => {
                    let indent = "  ".repeat(*depth);
                    text(
                        at(1, screen_row),
                        fit_cells(&format!("{indent}  {words}"), inner),
                        self.alphabet().edge.color,
                    );
                }
            }
        }

        // With no addressable row, the typing prompt becomes the one focus
        // signal. When a row exists its inversion is the signal instead.
        if browser.cursor().is_none() {
            let prompt = egui::Rect::from_min_size(at(1, 1), egui::vec2(cell.x, cell.y));
            painter.rect_filled(prompt, 0.0, self.focused());
            text(
                at(1, 1),
                browser::glyph::PROMPT.to_string(),
                self.alphabet().ground.color,
            );
            if self.browser.is_some() {
                crate::ui::nav_cursor::claim(
                    painter,
                    "stage-browser-prompt",
                    prompt,
                    crate::ui::nav_cursor::Kind::Prompt,
                    crate::ui::nav_cursor::Layer::Overlay,
                    alpha.focus.color,
                );
            }
        }
    }

    /// The codebook for the scope focus is standing in, drawn from the
    /// keymap table itself.
    ///
    /// Two columns, both left-aligned on their own axis so the eye reads
    /// down either one: keys at [`design::FOCUS`] because they are the
    /// actionable half, meanings at [`design::INK`]. Monospace does the
    /// alignment for free, which is most of why the whole stage is
    /// monospace.
    // SAMPLE-EDITOR-DRAW-BEGIN
    /// The cutting room. The field, whole, like the codebook: a casing
    /// with the file's name and facts and the three page tabs across
    /// the head; the waveform in a dark screen with the house chamfer,
    /// three tones deep; the markers, the slices, the cursor; a minimap
    /// of the whole file under it with the view's window drawn on;
    /// the page's readouts along the foot; and the keys that page
    /// answers to, in one line, last.
    fn draw_sample_editor(&self, painter: &egui::Painter, field: egui::Rect, phase: Phase) {
        use crate::params::sampler as sp;
        let Some((editor, device)) = self.edited_sampler() else {
            return;
        };
        let alpha = self.alphabet();
        let data = self.sample_data.as_ref();
        // The block face carries letters, digits and a few marks; a
        // file name's underscores and dots read as spaces on it.
        let name = device
            .sample
            .as_deref()
            .and_then(|path| path.file_stem())
            .map(|stem| {
                stem.to_string_lossy()
                    .to_uppercase()
                    .replace(['_', '.'], " ")
            })
            .unwrap_or_else(|| "NO FILE".to_owned());
        let seconds = data.map_or(0.0, SampleData::seconds);
        let word = |at: f64| sample::time_word(at, seconds);

        // ---- the casing
        let panel = field.shrink2(egui::vec2(28.0, 22.0));
        let inner = panel.shrink2(egui::vec2(24.0, 16.0));
        kit::cached(
            painter,
            egui::Id::new("stage-sample-shell"),
            panel,
            (alpha.surface.color, alpha.ground.color, alpha.ink.color),
            |out| {
                circuit::panel_variant(
                    out,
                    panel,
                    Some(alpha.surface.color),
                    alpha.ground.color,
                    Some((Weight::Heavy, alpha.ink.color)),
                    2,
                );
                circuit::panel_frame_variant(
                    out,
                    panel.shrink(5.0),
                    Weight::Hair,
                    alpha.edge.color,
                    1,
                );
            },
        );

        // ---- the head: sign, name, facts; the pages at the right
        const HEAD_H: f32 = 48.0;
        let head = egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), HEAD_H));
        let mut marks = Vec::new();
        Sign::Archive.paint(
            &mut marks,
            egui::Rect::from_center_size(
                egui::pos2(head.left() + 10.0, head.top() + 11.0),
                egui::Vec2::splat(20.0),
            ),
            Weight::Hair,
            alpha.edge.color,
        );
        painter.extend(marks);
        let shown_name: String = name.chars().take(28).collect();
        block::paint(
            painter,
            egui::Id::new("stage-sample-title"),
            egui::pos2(head.left() + 28.0, head.top()),
            egui::Align2::LEFT_TOP,
            block::unit::TITLE,
            &shown_name,
            alpha.ink.color,
        );
        let mode = sp::MODE_NAMES
            .get(device.value(sp::MODE).round() as usize)
            .copied()
            .unwrap_or("?");
        let facts = match data {
            Some(data) => format!(
                "{}  ·  {:.1}K  ·  {}CH  ·  {} SLICES  ·  {}",
                word(1.0),
                data.sample_rate as f32 / 1000.0,
                data.channels,
                device.slices.len(),
                mode.to_uppercase()
            ),
            None => format!(
                "LOADING  ·  {} SLICES  ·  {}",
                device.slices.len(),
                mode.to_uppercase()
            ),
        };
        painter.text(
            egui::pos2(head.left() + 28.0, head.top() + 28.0),
            egui::Align2::LEFT_TOP,
            facts,
            egui::FontId::monospace(11.0),
            alpha.edge.color,
        );
        // The pages: three words, the open one bracketed in the focus ink.
        let mut x = head.right();
        for page in SamplePage::ALL.iter().rev() {
            let on = *page == editor.page;
            let w = 62.0;
            x -= w;
            let tab = egui::Rect::from_min_size(
                egui::pos2(x, head.top() + 2.0),
                egui::vec2(w - 8.0, 20.0),
            );
            if on {
                let mut marks = Vec::new();
                circuit::brackets(&mut marks, tab, 5.0, Weight::Bold, alpha.focus.color);
                painter.extend(marks);
            }
            painter.text(
                tab.center(),
                egui::Align2::CENTER_CENTER,
                page.word(),
                egui::FontId::monospace(12.0),
                if on {
                    alpha.focus.color
                } else {
                    alpha.edge.color
                },
            );
        }

        // ---- the foot: readouts and the keys
        const FOOT_H: f32 = 56.0;
        const KEYS_H: f32 = 16.0;
        const MAP_H: f32 = 22.0;
        let keys_line = egui::Rect::from_min_max(
            egui::pos2(inner.left(), inner.bottom() - KEYS_H),
            inner.right_bottom(),
        );
        let foot = egui::Rect::from_min_max(
            egui::pos2(inner.left(), keys_line.top() - FOOT_H),
            egui::pos2(inner.right(), keys_line.top() - 4.0),
        );
        let map = egui::Rect::from_min_max(
            egui::pos2(inner.left(), foot.top() - MAP_H - 6.0),
            egui::pos2(inner.right(), foot.top() - 6.0),
        );
        // The slice list takes a column on the right on the SLICE page.
        let list_w = if editor.page == SamplePage::Slice {
            118.0
        } else {
            0.0
        };
        let screen = egui::Rect::from_min_max(
            egui::pos2(inner.left(), head.bottom() + 6.0),
            egui::pos2(inner.right() - list_w, map.top() - 8.0),
        );
        if screen.height() < 40.0 || screen.width() < 80.0 {
            return;
        }

        // ---- the screen
        let mut marks = Vec::new();
        circuit::panel_variant(
            &mut marks,
            screen,
            Some(alpha.ground.color),
            alpha.surface.color,
            Some((Weight::Hair, alpha.edge.color)),
            3,
        );
        painter.extend(marks);
        let wave = screen.shrink2(egui::vec2(6.0, 14.0));
        let x_of = |at: f64| -> f32 {
            let t = if editor.view_span > 0.0 {
                (at - editor.view_from) / editor.view_span
            } else {
                0.0
            };
            wave.left() + (t as f32) * wave.width()
        };
        let mid = wave.center().y;
        let half = wave.height() * 0.5 * 0.94;
        let start = f64::from(device.value(sp::START));
        let end = f64::from(device.value(sp::END));
        let (trim_a, trim_b) = if end > start {
            (start, end)
        } else {
            (start, 1.0)
        };
        let loop_at = f64::from(device.value(sp::LOOP_START));
        let visible = |at: f64| at >= editor.view_from - 1e-9 && at <= editor.view_to() + 1e-9;

        // The centre line, first and faintest.
        painter.line_segment(
            [egui::pos2(wave.left(), mid), egui::pos2(wave.right(), mid)],
            egui::Stroke::new(1.0, alpha.edge.color.gamma_multiply(0.6)),
        );

        match data {
            Some(data) => {
                // Outside the trim, the picture is a step dimmer: what
                // the sampler will not play is still there to see, but
                // it is not the subject.
                let columns = wave.width().max(1.0) as usize;
                let bins = data.peaks.columns(
                    Some(&data.samples),
                    editor.view_from,
                    editor.view_to(),
                    columns,
                );
                let per = wave.width() / columns as f32;
                for (i, bin) in bins.iter().enumerate() {
                    let x = wave.left() + (i as f32 + 0.5) * per;
                    let at =
                        editor.view_from + (i as f64 + 0.5) / columns as f64 * editor.view_span;
                    let inside = at >= trim_a && at <= trim_b;
                    let (outer, core) = if inside {
                        (alpha.edge.color, alpha.ink.color)
                    } else {
                        (alpha.edge.color.gamma_multiply(0.55), alpha.edge.color)
                    };
                    // Tone one: the extremes, a hairline the full reach.
                    let top = mid - bin.max.clamp(-1.0, 1.0) * half;
                    let bottom = mid - bin.min.clamp(-1.0, 1.0) * half;
                    painter.line_segment(
                        [egui::pos2(x, top.min(mid)), egui::pos2(x, bottom.max(mid))],
                        egui::Stroke::new(1.0, outer),
                    );
                    // Tone two: the density, a heavier bar over the RMS.
                    let rms = bin.rms.clamp(0.0, 1.0) * half;
                    if rms >= 0.5 {
                        painter.line_segment(
                            [egui::pos2(x, mid - rms), egui::pos2(x, mid + rms)],
                            egui::Stroke::new(per.max(1.0), core),
                        );
                    }
                    // Tone three: the crest, one bright point where the
                    // extreme stands well past the density.
                    if inside && bin.max - bin.rms > 0.35 {
                        painter.rect_filled(
                            egui::Rect::from_center_size(
                                egui::pos2(x, top),
                                egui::Vec2::splat(2.0),
                            ),
                            0.0,
                            alpha.focus.color,
                        );
                    }
                }
            }
            None => {
                painter.text(
                    wave.center(),
                    egui::Align2::CENTER_CENTER,
                    "· · ·   READING THE FILE   · · ·",
                    egui::FontId::monospace(12.0),
                    alpha.edge.color,
                );
            }
        }

        // The audition: the range that is sounding, washed in the live ink.
        if let Some((from, to)) = editor.playing {
            let a = x_of(from.max(editor.view_from));
            let b = x_of(to.min(editor.view_to()));
            if b > a {
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(a, wave.top()),
                        egui::pos2(b, wave.bottom()),
                    ),
                    0.0,
                    alpha.live_dim.color.gamma_multiply(0.18),
                );
            }
        }

        // The slices: hairlines with their numbers, and the one under
        // the cursor washed and numbered in the focus ink.
        let under = editor.slice_at(device);
        // Numbers are dropped, never overlapped: a slice whose number
        // would land on the last one's keeps its line and loses its
        // word, and the list on the SLICE page still names it.
        let mut last_number_x = f32::NEG_INFINITY;
        for (index, at) in device.slices.iter().enumerate() {
            let next = device.slices.get(index + 1).copied().unwrap_or(1.0);
            let on = under == Some(index);
            if on {
                let a = x_of(at.max(editor.view_from));
                let b = x_of(next.min(editor.view_to()));
                if b > a {
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(a, screen.top() + 2.0),
                            egui::pos2(b, screen.bottom() - 2.0),
                        ),
                        0.0,
                        alpha.focus.color.gamma_multiply(0.07),
                    );
                }
            }
            if !visible(*at) {
                continue;
            }
            let x = x_of(*at);
            let mut marks = Vec::new();
            circuit::trace(
                &mut marks,
                &[
                    egui::pos2(x, screen.top() + 2.0),
                    egui::pos2(x, screen.bottom() - 2.0),
                ],
                Weight::Hair,
                if on {
                    alpha.focus.color
                } else {
                    alpha.ink.color
                },
            );
            painter.extend(marks);
            if on || x - last_number_x >= 18.0 {
                painter.text(
                    egui::pos2(x + 3.0, screen.top() + 3.0),
                    egui::Align2::LEFT_TOP,
                    format!("{:02}", index + 1),
                    egui::FontId::monospace(10.0),
                    if on {
                        alpha.focus.color
                    } else {
                        alpha.edge.color
                    },
                );
                last_number_x = x;
            }
        }

        // The trim: brackets at the start and the end, heavy; the loop
        // start dashed. Outside the trim, a wash so the excluded part
        // reads as behind glass.
        for (at, inward, label) in [(trim_a, 1.0f32, "S"), (trim_b, -1.0f32, "E")] {
            if !visible(at) {
                continue;
            }
            let x = x_of(at);
            let mut marks = Vec::new();
            circuit::trace(
                &mut marks,
                &[
                    egui::pos2(x, screen.top() + 1.0),
                    egui::pos2(x, screen.bottom() - 1.0),
                ],
                Weight::Heavy,
                alpha.ink.color,
            );
            for y in [screen.top() + 1.0, screen.bottom() - 1.0] {
                circuit::trace(
                    &mut marks,
                    &[egui::pos2(x, y), egui::pos2(x + inward * 9.0, y)],
                    Weight::Heavy,
                    alpha.ink.color,
                );
            }
            painter.extend(marks);
            painter.text(
                egui::pos2(x + inward * 6.0, screen.bottom() - 4.0),
                if inward > 0.0 {
                    egui::Align2::LEFT_BOTTOM
                } else {
                    egui::Align2::RIGHT_BOTTOM
                },
                label,
                egui::FontId::monospace(10.0),
                alpha.ink.color,
            );
        }
        if device.value(sp::LOOP_MODE).round() >= 1.0 && visible(loop_at) {
            let x = x_of(loop_at);
            let mut marks = Vec::new();
            circuit::dashes(
                &mut marks,
                &[
                    egui::pos2(x, screen.top() + 1.0),
                    egui::pos2(x, screen.bottom() - 1.0),
                ],
                0.0,
                Weight::Hair,
                alpha.ink.color,
            );
            painter.extend(marks);
            painter.text(
                egui::pos2(x + 4.0, screen.bottom() - 4.0),
                egui::Align2::LEFT_BOTTOM,
                "L",
                egui::FontId::monospace(10.0),
                alpha.edge.color,
            );
        }

        // The cursor: bold, in the focus ink, with its time at the head.
        if visible(editor.cursor) {
            let x = x_of(editor.cursor);
            crate::ui::nav_cursor::claim(
                painter,
                "stage-sample-playhead-cursor",
                egui::Rect::from_min_max(
                    egui::pos2(x - 1.5, screen.top()),
                    egui::pos2(x + 1.5, screen.bottom()),
                ),
                crate::ui::nav_cursor::Kind::Playhead,
                crate::ui::nav_cursor::Layer::Overlay,
                alpha.focus.color,
            );
            let text = if data.is_some() {
                word(editor.cursor)
            } else {
                format!("{:.1}%", editor.cursor * 100.0)
            };
            let align = if x > screen.center().x {
                egui::Align2::RIGHT_TOP
            } else {
                egui::Align2::LEFT_TOP
            };
            painter.text(
                egui::pos2(
                    x + if x > screen.center().x { -8.0 } else { 8.0 },
                    screen.top() + 14.0,
                ),
                align,
                text,
                egui::FontId::monospace(11.0),
                alpha.focus.color,
            );
        }

        // ---- the slice list, on the SLICE page
        if editor.page == SamplePage::Slice {
            let list = egui::Rect::from_min_max(
                egui::pos2(screen.right() + 8.0, screen.top()),
                egui::pos2(inner.right(), screen.bottom()),
            );
            let mut marks = Vec::new();
            circuit::panel_frame_variant(&mut marks, list, Weight::Hair, alpha.edge.color, 0);
            painter.extend(marks);
            block::paint(
                painter,
                egui::Id::new("stage-sample-list-title"),
                egui::pos2(list.left() + 8.0, list.top() + 6.0),
                egui::Align2::LEFT_TOP,
                block::unit::MICRO,
                "SLICES",
                alpha.edge.color,
            );
            let row_h = 15.0;
            let rows = ((list.height() - 26.0) / row_h).floor().max(0.0) as usize;
            let total = device.slices.len();
            let first = under
                .map(|u| u.saturating_sub(rows / 2))
                .unwrap_or(0)
                .min(total.saturating_sub(rows));
            for (shown, index) in (first..total.min(first + rows)).enumerate() {
                let y = list.top() + 26.0 + shown as f32 * row_h;
                let on = under == Some(index);
                painter.text(
                    egui::pos2(list.left() + 8.0, y),
                    egui::Align2::LEFT_TOP,
                    format!("{:02}  {}", index + 1, word(device.slices[index])),
                    egui::FontId::monospace(11.0),
                    if on {
                        alpha.focus.color
                    } else {
                        alpha.ink.color
                    },
                );
            }
            if total == 0 {
                painter.text(
                    egui::pos2(list.left() + 8.0, list.top() + 26.0),
                    egui::Align2::LEFT_TOP,
                    "none yet",
                    egui::FontId::monospace(11.0),
                    alpha.edge.color,
                );
            }
        }

        // ---- the minimap: the whole file, and the window on it
        let mut marks = Vec::new();
        circuit::panel_frame_variant(&mut marks, map, Weight::Hair, alpha.edge.color, 2);
        painter.extend(marks);
        let map_wave = map.shrink2(egui::vec2(4.0, 3.0));
        if let Some(data) = data {
            let columns = map_wave.width().max(1.0) as usize;
            let bins = data.peaks.columns(None, 0.0, 1.0, columns);
            let per = map_wave.width() / columns as f32;
            let mid = map_wave.center().y;
            let half = map_wave.height() * 0.5;
            for (i, bin) in bins.iter().enumerate() {
                let x = map_wave.left() + (i as f32 + 0.5) * per;
                let reach = bin.max.abs().max(bin.min.abs()).clamp(0.0, 1.0) * half;
                painter.line_segment(
                    [egui::pos2(x, mid - reach), egui::pos2(x, mid + reach)],
                    egui::Stroke::new(1.0, alpha.edge.color),
                );
            }
        }
        for at in &device.slices {
            let x = map_wave.left() + (*at as f32) * map_wave.width();
            painter.line_segment(
                [
                    egui::pos2(x, map.top() + 1.0),
                    egui::pos2(x, map.top() + 5.0),
                ],
                egui::Stroke::new(1.0, alpha.ink.color),
            );
        }
        let window = egui::Rect::from_min_max(
            egui::pos2(
                map_wave.left() + editor.view_from as f32 * map_wave.width(),
                map.top() + 1.0,
            ),
            egui::pos2(
                map_wave.left() + editor.view_to() as f32 * map_wave.width(),
                map.bottom() - 1.0,
            ),
        );
        let mut marks = Vec::new();
        circuit::brackets(&mut marks, window, 4.0, Weight::Heavy, alpha.focus.color);
        painter.extend(marks);
        let cursor_x = map_wave.left() + editor.cursor as f32 * map_wave.width();
        painter.line_segment(
            [
                egui::pos2(cursor_x, map.top() + 2.0),
                egui::pos2(cursor_x, map.bottom() - 2.0),
            ],
            egui::Stroke::new(1.0, alpha.focus.color),
        );

        // ---- the readouts, per page
        let value_of = |param: u32| -> String {
            let spec = device.kind.spec();
            spec.params
                .iter()
                .zip(spec.labels)
                .find(|(def, _)| def.id == param)
                .map(|(def, label)| chain::format_param(def, label, device.value(param)))
                .unwrap_or_default()
        };
        let readouts: Vec<(&str, String)> = match editor.page {
            SamplePage::Trim => vec![
                ("START", word(start)),
                ("END", word(if end > start { end } else { 1.0 })),
                ("LOOP", word(loop_at)),
                ("LOOP MODE", value_of(sp::LOOP_MODE).to_uppercase()),
                ("FADE IN", value_of(sp::FADE_IN)),
                ("FADE OUT", value_of(sp::FADE_OUT)),
            ],
            SamplePage::Slice => vec![
                ("SLICES", device.slices.len().to_string()),
                ("GRID", editor.count.to_string()),
                ("ONSETS", format!("{:.0}%", editor.sensitivity * 100.0)),
                ("SNAP", if editor.snap { "ZERO" } else { "FREE" }.to_owned()),
                ("MODE", mode.to_uppercase()),
                ("CHOKE", value_of(sp::CHOKE).to_uppercase()),
            ],
            SamplePage::Attr => vec![
                ("GAIN", value_of(sp::GAIN)),
                ("TUNE", value_of(sp::TUNE)),
                ("ROOT", value_of(sp::ROOT)),
                (
                    "REVERSE",
                    if device.value(sp::REVERSE).round() >= 1.0 {
                        "ON"
                    } else {
                        "OFF"
                    }
                    .to_owned(),
                ),
                ("MODE", mode.to_uppercase()),
                ("LOOP MODE", value_of(sp::LOOP_MODE).to_uppercase()),
            ],
        };
        let cell_w = foot.width() / readouts.len().max(1) as f32;
        for (index, (label, value)) in readouts.iter().enumerate() {
            let cell = egui::Rect::from_min_size(
                egui::pos2(foot.left() + index as f32 * cell_w, foot.top()),
                egui::vec2(cell_w - 6.0, foot.height()),
            );
            let mut marks = Vec::new();
            circuit::panel_frame_variant(
                &mut marks,
                cell,
                Weight::Hair,
                alpha.edge.color,
                (index % 4) as u8,
            );
            painter.extend(marks);
            block::paint(
                painter,
                egui::Id::new(("stage-sample-readout", index, *label)),
                egui::pos2(cell.left() + 8.0, cell.top() + 7.0),
                egui::Align2::LEFT_TOP,
                block::unit::MICRO,
                label,
                alpha.edge.color,
            );
            painter.text(
                egui::pos2(cell.left() + 8.0, cell.bottom() - 7.0),
                egui::Align2::LEFT_BOTTOM,
                value,
                egui::FontId::monospace(13.0),
                alpha.ink.color,
            );
        }

        // ---- the keys this page answers to
        let keys = match editor.page {
            SamplePage::Trim => {
                "S START  E END  L LOOP  Z SNAP  P PLAY  +P ALL  UP/DOWN ZOOM  PGUP/PGDN SCROLL  ^ARROWS MARKERS  TAB PAGE  ESC OUT"
            }
            SamplePage::Slice => {
                "ENTER CUT  DEL REMOVE  G GRID  +/- GRID SIZE  T ONSETS  [ ] SENSITIVITY  C CLEAR  P PLAY SLICE  TAB PAGE"
            }
            SamplePage::Attr => {
                "N NORMALIZE  +/- GAIN  R REVERSE  M MODE  Q LOOP MODE  P PLAY  ^Z UNDO  TAB PAGE  ESC OUT"
            }
        };
        painter.text(
            keys_line.left_center(),
            egui::Align2::LEFT_CENTER,
            keys,
            egui::FontId::monospace(10.0),
            alpha.edge.color,
        );
        let _ = phase;
    }
    // SAMPLE-EDITOR-DRAW-END

    fn draw_help(&self, painter: &egui::Painter, field: egui::Rect) {
        let scope = self.scope_context();
        let mut rows: Vec<(String, &'static str)> = keymap::bindings_for(scope)
            .map(|(modifiers, key, intent)| (keymap::chord_name(modifiers, key), intent.label()))
            .collect();
        if scope == keymap::ScopeContext::Clip {
            rows.extend(
                CLIP_HELP_EXAMPLES
                    .into_iter()
                    .map(|(chord, label)| (chord.to_owned(), label)),
            );
        }
        if rows.is_empty() {
            return;
        }

        let alpha = self.alphabet();
        let panel = field.shrink2(egui::vec2(28.0, 22.0));
        let header_h = 42.0;
        let footer_h = 22.0;
        let pitch = 21.0;
        let inner = panel.shrink2(egui::vec2(26.0, 18.0));
        let available_h = (inner.height() - header_h - footer_h).max(pitch);
        let max_rows = (available_h / pitch).floor().max(1.0) as usize;
        let columns = rows.len().div_ceil(max_rows).clamp(1, 3);
        let per_column = rows.len().div_ceil(columns);
        let column_w = inner.width() / columns as f32;

        kit::cached(
            painter,
            egui::Id::new(("stage-help-shell", scope as u8)),
            panel,
            (alpha.surface.color, alpha.ground.color, alpha.ink.color),
            |out| {
                circuit::panel_variant(
                    out,
                    panel,
                    Some(alpha.surface.color),
                    alpha.ground.color,
                    Some((Weight::Heavy, alpha.ink.color)),
                    3,
                );
                circuit::panel_frame_variant(
                    out,
                    panel.shrink(5.0),
                    Weight::Hair,
                    alpha.edge.color,
                    0,
                );
                let sign = egui::Rect::from_center_size(
                    egui::pos2(inner.left() + 12.0, inner.top() + 10.0),
                    egui::Vec2::splat(18.0),
                );
                Sign::Codex.paint(out, sign, Weight::Hair, alpha.edge.color);
                circuit::rail(
                    out,
                    egui::pos2(inner.left() + 30.0, inner.top() + 10.0),
                    egui::pos2(inner.right(), inner.top() + 10.0),
                    &[0.0, 0.72, 1.0],
                    alpha.edge.color,
                );
            },
        );
        block::paint(
            painter,
            egui::Id::new(("stage-help-title", scope as u8)),
            egui::pos2(inner.left() + 34.0, inner.top()),
            egui::Align2::LEFT_TOP,
            block::unit::TITLE,
            "CODEX",
            alpha.ink.color,
        );
        crate::ui::nav_cursor::claim(
            painter,
            "stage-help-cursor",
            egui::Rect::from_min_size(inner.min, egui::vec2(inner.width(), header_h)),
            crate::ui::nav_cursor::Kind::Prompt,
            crate::ui::nav_cursor::Layer::Overlay,
            alpha.focus.color,
        );

        for (index, (chord, label)) in rows.iter().enumerate() {
            let column = index / per_column;
            if column >= columns {
                break;
            }
            let row = index % per_column;
            let left = inner.left() + column as f32 * column_w;
            let y = inner.top() + header_h + row as f32 * pitch;
            let chord = carved_chord(chord);
            block::paint(
                painter,
                egui::Id::new(("stage-help-chord", scope as u8, index)),
                egui::pos2(left, y),
                egui::Align2::LEFT_TOP,
                block::unit::MICRO,
                &chord,
                alpha.focus.color,
            );
            painter.text(
                egui::pos2(
                    left + column_w * 0.36,
                    y + block::height(block::unit::MICRO) * 0.5,
                ),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::monospace(13.0),
                alpha.ink.color,
            );
        }

        painter.text(
            egui::pos2(inner.left(), inner.bottom()),
            egui::Align2::LEFT_BOTTOM,
            "CHORD / MEANING   ·   ? CLOSES THE CODEX",
            egui::FontId::monospace(11.0),
            alpha.edge.color,
        );
    }

    fn draw_message(&self, painter: &egui::Painter, zone: egui::Rect) {
        // The badge and the colophon take the strip's left shoulder, so
        // everything the strip has to SAY begins after them.
        const MARGIN: f32 = 190.0;

        let Some(refusal) = self.refusal else {
            // With no refusal this frame, the strip carries the
            // sequencer's last notice and the pitch-entry mode: a mode
            // must announce itself, and a refused edit must be read.
            let mut words = Vec::new();
            if self.midi_typing.enabled() {
                words.push("MIDI · letters are pitches");
            }
            // A mode that does not announce itself is a trap, and a
            // rename is a mode: every letter is the name until Enter.
            let renaming = self
                .renaming
                .as_ref()
                .map(|rename| format!("RENAME · {}_", rename.text));
            if let Some(renaming) = &renaming {
                words.push(renaming.as_str());
            } else if let Some(notice) = &self.notice {
                words.push(notice.as_str());
            }
            // A render under way: a gauge at the strip's right shoulder,
            // filling as it goes.
            if let Some(export) = self.export_state() {
                let gauge = egui::Rect::from_min_max(
                    egui::pos2(zone.max.x - 260.0, zone.center().y - 5.0),
                    egui::pos2(zone.max.x - 120.0, zone.center().y + 5.0),
                );
                let mut marks = Vec::new();
                circuit::tick_bar(
                    &mut marks,
                    gauge,
                    20,
                    export.progress,
                    self.alphabet().live.color,
                    self.alphabet().edge.color,
                    true,
                );
                painter.extend(marks);
                painter.text(
                    egui::pos2(gauge.min.x - 8.0, zone.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{:>3}%", (export.progress * 100.0).round() as u32),
                    egui::FontId::monospace(design::px(design::type_scale::MICRO)),
                    self.alphabet().ink.color,
                );
            }
            if !words.is_empty() {
                let mut marks = Vec::new();
                let pad = egui::pos2(zone.min.x + MARGIN - 13.0, zone.center().y);
                circuit::pad(
                    &mut marks,
                    pad,
                    circuit::PAD,
                    self.alphabet().edge.color,
                    true,
                );
                circuit::trace(
                    &mut marks,
                    &[pad, pad + egui::vec2(8.0, 0.0)],
                    Weight::Hair,
                    self.alphabet().edge.color,
                );
                painter.extend(marks);
                painter.text(
                    egui::pos2(zone.min.x + MARGIN, zone.center().y),
                    egui::Align2::LEFT_CENTER,
                    words.join("   "),
                    egui::FontId::monospace(16.0),
                    self.alphabet().ink.color,
                );
            }
            return;
        };
        let words = match refusal.reason {
            RefusalReason::Edge(Step::Up) => "Refused · edge up",
            RefusalReason::Edge(Step::Down) => "Refused · edge down",
            RefusalReason::Edge(Step::Left) => "Refused · edge left",
            RefusalReason::Edge(Step::Right) => "Refused · edge right",
            RefusalReason::Deeper => "Refused · depth limit",
            RefusalReason::Shallower => "Refused · no further out",
            RefusalReason::Empty => "Refused · nothing here yet",
            RefusalReason::AtTop => "Refused · already at top",
            RefusalReason::Unavailable => "Refused · no action yet",
        };
        let mut marks = Vec::new();
        let pad = egui::pos2(zone.min.x + MARGIN - 13.0, zone.center().y);
        circuit::pad(
            &mut marks,
            pad,
            circuit::PAD + 2.0,
            self.refusal_ink(),
            true,
        );
        circuit::trace(
            &mut marks,
            &[pad, pad + egui::vec2(8.0, 0.0)],
            Weight::Bold,
            self.refusal_ink(),
        );
        painter.extend(marks);
        painter.text(
            egui::pos2(zone.min.x + MARGIN, zone.center().y),
            egui::Align2::LEFT_CENTER,
            words,
            egui::FontId::monospace(16.0),
            self.refusal_ink(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{command, drive, into_clip, into_field};
    use super::*;

    #[test]
    fn the_field_shows_as_many_columns_as_its_own_geometry_allows() {
        let gap = column_gap();
        let margin = design::px(design::space::ROOM);
        // Exactly the width the strip's own drawing would need for `n`
        // tracks — plus the master's column, which is pinned to the right
        // and is never the strip's to give away.
        let width_for =
            |n: f32| n * TRACK_W + (n - 1.0) * gap + margin * 2.0 + ADDRESS_W + TRACK_W + gap;
        let field = |w: f32| egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, 400.0));

        for n in 1..=5 {
            assert_eq!(
                Stage::strip_capacity(field(width_for(n as f32))),
                n,
                "a field sized for {n} columns did not offer {n}"
            );
            assert_eq!(
                Stage::strip_capacity(field(width_for(n as f32) - 1.0)),
                (n - 1).max(1),
                "a field one pixel short of {n} columns still offered {n}"
            );
        }
    }

    #[test]
    fn a_field_too_narrow_for_any_column_still_offers_one() {
        let cramped = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(4.0, 400.0));
        assert_eq!(
            Stage::strip_capacity(cramped),
            1,
            "the cursor was left with nowhere to stand"
        );
    }

    /// Only one cursor is focus-bright: while the band holds the keys
    /// the session's cursor rests, and it comes back bright when the
    /// band closes.
    #[test]
    fn the_session_cursor_rests_while_the_band_holds_the_keys() {
        let mut stage = Stage::new();
        stage
            .song
            .add_device(0, crate::devices::DeviceKind::Sat)
            .expect("effect");
        let bright = stage.session_lattice().map(|(_, shade)| shade);
        assert_eq!(bright, Some(stage.focused()));
        assert_eq!(command(&mut stage, Key::D), ApplyOutcome::Changed);
        let resting = stage.session_lattice().map(|(_, shade)| shade);
        assert_eq!(
            resting,
            Some(stage.resting()),
            "two cursors were bright at once"
        );
        assert_eq!(
            drive(&mut stage, &[Key::Escape]),
            vec![ApplyOutcome::Changed]
        );
        assert_eq!(stage.session_lattice().map(|(_, shade)| shade), bright);
    }

    #[test]
    fn the_clip_tray_is_a_fixed_band_at_the_foot_of_the_field() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let layout = Layout::of(window);
        assert_eq!(layout.clip.height(), CLIP_H);
        assert_eq!(
            layout.clip.max.y, layout.field.max.y,
            "the tray left the foot"
        );
        assert_eq!(
            layout.session.max.y, layout.clip.min.y,
            "session and tray overlap or gap"
        );
        assert_eq!(layout.session.min, layout.field.min);
        assert_eq!(
            layout.session.height() + layout.clip.height(),
            layout.field.height(),
            "the two do not partition the field"
        );
    }

    #[test]
    fn the_session_stays_drawn_while_a_clip_is_open_with_a_resting_cursor() {
        let mut stage = Stage::new();
        assert_eq!(
            stage.session_lattice().map(|(_, shade)| shade),
            Some(stage.focused())
        );
        into_clip(&mut stage);
        assert_eq!(
            stage.session_lattice().map(|(_, shade)| shade),
            Some(stage.resting()),
            "the session vanished, or kept a focus-bright cursor, under an open clip"
        );
        drive(&mut stage, &[Key::Escape]);
        assert_eq!(
            stage.session_lattice().map(|(_, shade)| shade),
            Some(stage.focused())
        );
        // Inside a TRACK the field is the calibration grid, not the session.
        drive(&mut stage, &[Key::ArrowUp]);
        into_field(&mut stage);
        assert_eq!(stage.session_lattice(), None);
    }

    /// The guarantee, held as code: NOTHING the stage can be doing moves
    /// a zone. Layout is a function of the window alone, so summoning the
    /// browser, opening the codebook, descending, or being refused all
    /// leave every other zone exactly where the eye last found it.
    #[test]
    fn no_state_the_stage_can_reach_moves_a_zone() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let reference = Layout::of(window);

        let mut stage = Stage::new();
        for chord in [
            (Mods::COMMAND, Key::F),
            (Mods::NONE, Key::Questionmark),
            (Mods::NONE, Key::Enter),
            (Mods::NONE, Key::ArrowUp),
            (Mods::NONE, Key::Escape),
            (Mods::NONE, Key::Escape),
            (Mods::COMMAND, Key::F),
        ] {
            let _ = stage.handle_key(chord.0, chord.1);
            assert_eq!(
                Layout::of(window),
                reference,
                "a zone moved because of what the stage was doing"
            );
        }
    }

    /// The browser opens OVER the field, so the field is laid out as
    /// though it did not exist and gets its whole self back the moment
    /// the browser closes.
    #[test]
    fn the_browser_overlays_the_field_rather_than_dividing_it() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let layout = Layout::of(window);

        assert_eq!(layout.browser.width(), BROWSER_W);
        assert_eq!(
            layout.field.min.x, layout.browser.min.x,
            "the field yielded ground to the browser"
        );
        assert_eq!(layout.field.width(), window.width() - 2.0 * FRAME_W);
        assert!(layout.field.contains_rect(layout.browser));
    }

    /// The periphery is one casing: the strips span the window, and the
    /// field stands off both sides by the same rail, so surface material
    /// runs unbroken from the vitals down either side into the message.
    #[test]
    /// The screen chamfer sits on the window's own corner, at its size,
    /// and never grows past a window too small to hold it.
    #[test]
    fn the_screen_chamfer_cuts_the_top_left_corner_at_its_size() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let [corner, along, down] = screen_chamfer_cut(window);
        assert_eq!(corner, window.min);
        assert_eq!(along, egui::pos2(SCREEN_CHAMFER, 0.0));
        assert_eq!(down, egui::pos2(0.0, SCREEN_CHAMFER));
        assert!(
            SCREEN_CHAMFER < PERIPHERY_H,
            "the cut would leave the vitals strip"
        );
        let tiny = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(10.0, 30.0));
        let [_, along, down] = screen_chamfer_cut(tiny);
        assert_eq!(along.x, 10.0);
        assert_eq!(down.y, 10.0);
    }

    /// The foot's cuts sit on their own corners, much smaller than the
    /// crown's, and every cut's apex is a corner of the window with its
    /// face's ends on the window's edges.
    #[test]
    fn the_foot_is_chamfered_small_on_both_sides() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        assert!(
            SCREEN_CHAMFER_FOOT * 2.0 < SCREEN_CHAMFER,
            "the foot's cuts are not much smaller"
        );
        let f = SCREEN_CHAMFER_FOOT;
        assert_eq!(
            screen_cut(window, ScreenCorner::BottomLeft),
            [
                egui::pos2(0.0, 800.0),
                egui::pos2(f, 800.0),
                egui::pos2(0.0, 800.0 - f)
            ]
        );
        assert_eq!(
            screen_cut(window, ScreenCorner::BottomRight),
            [
                egui::pos2(1280.0, 800.0),
                egui::pos2(1280.0 - f, 800.0),
                egui::pos2(1280.0, 800.0 - f)
            ]
        );
        for corner in ScreenCorner::ALL {
            let [apex, along, down] = screen_cut(window, corner);
            let on_corner =
                (apex.x == 0.0 || apex.x == 1280.0) && (apex.y == 0.0 || apex.y == 800.0);
            assert!(on_corner, "{corner:?} apex {apex:?} is not a window corner");
            assert_eq!(
                along.y, apex.y,
                "{corner:?} face does not start on the edge"
            );
            assert_eq!(down.x, apex.x, "{corner:?} face does not end on the edge");
        }
    }

    /// The rings nest inside the cut, each strictly inside the last and
    /// all inside the cut face, and a window too small for them shows
    /// fewer rather than a tangle.
    #[test]
    fn the_chamfer_rings_nest_inside_the_cut() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let rings = screen_chamfer_rings(window);
        assert_eq!(
            rings.len(),
            CHAMFER_RINGS,
            "a ring did not fit at full size"
        );
        let mut last_leg = SCREEN_CHAMFER;
        for (n, tri) in rings.iter().enumerate() {
            let d = CHAMFER_RING_GAP * (n + 1) as f32;
            assert_eq!(tri[0], egui::pos2(d, d), "ring {n} lost its right angle");
            let leg = tri[1].x - tri[0].x;
            assert!(leg < last_leg, "ring {n} is not inside the last");
            // On the cut face x + y = C, so every vertex sits at or
            // under it by at least the gap along the normal.
            for p in tri {
                assert!(
                    p.x + p.y <= SCREEN_CHAMFER - CHAMFER_RING_GAP,
                    "ring {n} crossed the cut"
                );
            }
            last_leg = leg;
        }
        let tiny = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(14.0, 14.0));
        assert!(screen_chamfer_rings(tiny).len() < CHAMFER_RINGS);
    }

    /// The stream screen sits inside the vitals strip, clear of the
    /// register rail that precedes the transport, at its own size.
    #[test]
    fn the_stream_screen_sits_in_the_strip_before_the_register() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let layout = Layout::of(window);
        let screen = stream_screen(layout.vitals, layout.transport);
        assert!(
            layout.vitals.contains_rect(screen),
            "the screen left the strip"
        );
        let room = design::px(design::space::ROOM);
        assert!(
            screen.right() <= layout.transport.min.x - room * 12.0,
            "the screen overlaps the register"
        );
        assert_eq!(screen.width(), STREAM_SCREEN_W);
        assert_eq!(screen.height(), STREAM_SCREEN_H);
    }

    #[test]
    fn the_field_is_framed_on_all_four_sides() {
        let window = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 800.0));
        let layout = Layout::of(window);

        assert_eq!(
            layout.vitals.width(),
            window.width(),
            "the vitals strip stopped short"
        );
        assert_eq!(
            layout.message.width(),
            window.width(),
            "the message strip stopped short"
        );
        assert_eq!(
            layout.field.min.x - window.min.x,
            FRAME_W,
            "the left rail is not the frame"
        );
        assert_eq!(
            window.max.x - layout.field.max.x,
            FRAME_W,
            "the right rail is not the frame"
        );
        assert_eq!(layout.field.min.y, layout.vitals.max.y);
        assert_eq!(layout.field.max.y, layout.message.min.y);
    }

    /// The point of a polarity rather than a second palette: every mark
    /// follows the ground, and none of them is a colour named at compile
    /// time that would stay put while the rest turned over.
    #[test]
    fn every_mark_follows_the_ground() {
        let mut stage = Stage::new();
        let dark = (
            stage.square(),
            stage.focused(),
            stage.resting(),
            stage.refusal_ink(),
            stage.veil(),
            stage.alphabet().live.color,
        );
        let _ = command(&mut stage, Key::L);
        let light = (
            stage.square(),
            stage.focused(),
            stage.resting(),
            stage.refusal_ink(),
            stage.veil(),
            stage.alphabet().live.color,
        );
        assert_ne!(
            dark.0, light.0,
            "the resting plane did not follow the ground"
        );
        assert_ne!(dark.1, light.1, "focus did not follow the ground");
        assert_ne!(dark.2, light.2);
        assert_ne!(dark.3, light.3);
        assert_ne!(
            dark.4, light.4,
            "the veil pulls the same way on both grounds"
        );
        assert_ne!(
            dark.5, light.5,
            "the sounding present did not follow the ground"
        );
    }

    #[test]
    fn the_outer_shell_stays_below_every_working_plane() {
        for polarity in [design::Polarity::Dark, design::Polarity::Light] {
            let alpha = design::Alphabet::for_polarity(polarity);
            let base = design::lightness_of(shell_base(polarity));
            let plate = design::lightness_of(shell_plate(polarity));
            let ground = design::lightness_of(alpha.ground.color);
            let surface = design::lightness_of(alpha.surface.color);

            assert!(base < plate, "the casing did not sit below its faceplate");
            assert!(base < ground, "the casing did not sit below the field");
            assert!(plate < surface, "the faceplate competed with a card");
        }
    }

    #[test]
    fn the_master_is_pinned_right_however_far_the_strip_scrolls() {
        let field = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1280.0, 400.0));
        let master = Stage::master_rect(field);
        let margin = design::px(design::space::ROOM);
        assert!(
            (master.max.x - (field.max.x - margin)).abs() < 0.5,
            "the master is not against the right edge"
        );
        // And the strip never reaches it: the last column the strip can
        // draw ends before the master begins.
        let last = Stage::strip_capacity(field).saturating_sub(1);
        assert!(
            Stage::head_rect(field, last).max.x <= master.min.x,
            "a track was drawn under the master"
        );
    }

    #[test]
    fn the_board_has_one_bus_per_shown_column() {
        let field = egui::Rect::from_min_size(egui::pos2(12.0, 24.0), egui::vec2(1280.0, 400.0));
        for slot in 0..Stage::strip_capacity(field) {
            assert_eq!(bus_x(field, slot), Stage::head_rect(field, slot).center().x);
            if slot > 0 {
                assert_eq!(
                    bus_x(field, slot) - bus_x(field, slot - 1),
                    TRACK_W + column_gap(),
                    "adjacent buses lost the column pitch"
                );
            }
        }
    }
}
