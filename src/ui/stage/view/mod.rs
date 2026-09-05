//! The view: an empty window.
//!
//! The canvas the next look is drawn on. Everything the stage IS still
//! runs behind this glass — every key goes through the codebook, the
//! machine room takes the keyboard when summoned and gives it back, the
//! clock ticks — and nothing is painted but the ground. What was drawn
//! before is on `mvp-port` at 2ee4b64, the worked example of the contract
//! in `notes/20260905-ui-seam-contract.md`.
//!
//! Every surface the inventory names is on this glass now, drawn in the
//! console's chassis and palette; the sequencer and the command palette
//! are the shared widgets, lifted through the palette and pumped here.

mod band;
mod browser;
mod callouts;
mod chassis;
mod faces;
mod heads;
mod help;
mod input;
mod inspector;
mod lattice;
mod log;
mod mixer;
mod palette;
mod room;
mod sample;
mod song;
mod status;
mod strip;
mod telemetry;
mod tray;
mod utility;

use super::key::{Key, Mods};
use super::*;
use crate::design::codex::Sign;
use crate::design::kit::{self, Weight};
use crate::design::motion::{self, Phase};
use crate::design::{self, block};
use crate::ui::chrome;
use eframe::egui;

/// An axis nothing is drawn along yet can hold everything: no offset
/// needs to move to keep the cursor in sight.
const HOLDS_EVERYTHING: usize = usize::MAX;

/// The window carved: a title row, the field, a status strip.
struct Layout {
    title: egui::Rect,
    field: egui::Rect,
    tray: egui::Rect,
    status: egui::Rect,
}

impl Layout {
    fn of(whole: egui::Rect) -> Self {
        let title_h = status::title_h();
        let status_h = status::status_h();
        let title =
            egui::Rect::from_min_max(whole.min, egui::pos2(whole.max.x, whole.min.y + title_h));
        let status =
            egui::Rect::from_min_max(egui::pos2(whole.min.x, whole.max.y - status_h), whole.max);
        let band = egui::Rect::from_min_max(title.left_bottom(), status.right_top());
        // The tray is cut off the FOOT of the band, fixed: the session
        // above keeps its shape whether or not the tray has a clip.
        let tray_top = (band.max.y - tray::tray_h()).max(band.min.y);
        let field = egui::Rect::from_min_max(band.min, egui::pos2(band.max.x, tray_top));
        let tray = egui::Rect::from_min_max(egui::pos2(band.min.x, tray_top), band.max);
        Self {
            title,
            field,
            tray,
            status,
        }
    }
}

/// The watched overrides, made on first use.
fn overrides() -> std::sync::MutexGuard<'static, crate::tune::Overrides> {
    static O: std::sync::OnceLock<std::sync::Mutex<crate::tune::Overrides>> =
        std::sync::OnceLock::new();
    O.get_or_init(|| {
        std::sync::Mutex::new(crate::tune::Overrides::new(crate::tune::overrides_path()))
    })
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The inspector, made on first use.
fn inspector() -> std::sync::MutexGuard<'static, inspector::Inspector> {
    static I: std::sync::OnceLock<std::sync::Mutex<inspector::Inspector>> =
        std::sync::OnceLock::new();
    I.get_or_init(|| std::sync::Mutex::new(inspector::Inspector::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The log and the trace, made on first use.
fn telemetry() -> std::sync::MutexGuard<'static, telemetry::Telemetry> {
    static T: std::sync::OnceLock<std::sync::Mutex<telemetry::Telemetry>> =
        std::sync::OnceLock::new();
    T.get_or_init(|| std::sync::Mutex::new(telemetry::Telemetry::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The watched theme, made on first use.
fn skin() -> std::sync::MutexGuard<'static, palette::Skin> {
    static SKIN: std::sync::OnceLock<std::sync::Mutex<palette::Skin>> = std::sync::OnceLock::new();
    SKIN.get_or_init(|| std::sync::Mutex::new(palette::Skin::new(palette::theme_path())))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Stage {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        // One keyboard cursor for the whole application: surfaces claim
        // the rectangle they address, and the overlay owns the mark.
        crate::ui::nav_cursor::configure(
            ui.ctx(),
            self.utility.prefs().reduced_motion,
            self.utility.prefs().cursor_energy,
        );
        crate::ui::nav_cursor::begin_frame(ui.ctx());
        // The theme file, once a frame: an edit from the picker lands on
        // the next frame, and a frame is asked for so it shows.
        if skin().poll() | overrides().poll() {
            ui.ctx().request_repaint();
        }
        // The backtick opens the knobs. A view key, not the codebook's:
        // it is about the glass, not the song.
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Backtick)) {
            let mut i = inspector();
            i.open = !i.open;
            if i.open {
                i.rescan();
            }
        }
        self.begin_frame();
        if self.poll_library() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        // A utility room is a true modal: it gets the frame's keyboard
        // before the musical surface and may close itself with Escape.
        self.update_utility(ui.ctx());
        let utility_open = self.utility.is_open();

        // `:` summons the palette. Checked before anything else reads the
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
        // While the palette is open it owns the keyboard OUTRIGHT.
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
                        // A physical '?' is the codebook chord and egui also
                        // emits it as text; admit each keystroke once.
                        if questionmark_consumed && text == "?" {
                            continue;
                        }
                        if space_consumed && text == " " {
                            continue;
                        }
                        stage_inputs.extend(text.chars().map(keymap::StageInput::Text));
                    }
                }
                stage_inputs
            })
        };
        let bound: Vec<keymap::StageInput> = inputs.clone();
        self.take_inputs(inputs, selection_held, selection_pressed);

        // Inside a clip, the letters may be pitches. Read after the
        // stage's own chords, so `^T` is never read as a T.
        let update = self
            .pitch_entry_mode()
            .map(|mode| self.midi_typing.update(ui.ctx(), mode));
        let enter_held = ui.input(|input| input.key_down(egui::Key::Enter));
        self.take_pitch_entry(update, enter_held);

        let layout = Layout::of(ui.available_rect_before_wrap());
        let field = layout.field;
        self.follow_cursor(
            heads::capacity(field.width()),
            lattice::capacity(field),
            Self::chain_capacity(layout.tray),
        );

        let dt = ui.ctx().input(|input| input.stable_dt);
        self.tick_clock(dt);
        // The alarm on the glass: an xrun's flash, for as long as vitals
        // holds it.
        crate::shell::post::set_alarm(if self.vitals.flashing() { 1.0 } else { 0.0 });
        // The log and the trace: what changed this frame, and the mix.
        {
            let master = self.meters.master().level;
            let facts = telemetry::Was_::new(
                self.transport.motion(),
                &self.playing,
                self.inside.map(|o| (o.pattern.0, o.track)),
                self.chain.is_some(),
                self.browser.is_some(),
                self.help,
                self.mixing,
                self.song_view,
                self.sample.is_some(),
                self.utility.is_open(),
                self.dirty,
                self.song.tracks.len(),
                self.song.session.scenes.len(),
                self.refusal
                    .as_ref()
                    .map(|r| format!("{:?}", r.reason).to_ascii_lowercase()),
                self.notice.clone(),
            );
            telemetry().observe(dt, &bound, facts, master.left.max(master.right));
        }
        if self.wants_repaint() {
            ui.ctx().request_repaint();
        }

        self.draw(ui);
    }

    /// Pump the command palette while it is open; a chosen command comes
    /// back as the intent it names. The palette is stock chrome, so it
    /// reads the runtime theme rather than the console's palette.
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
        let theme = palette::theme();
        let choice = self.palette.show(ctx, &theme, &commands, &[])?;
        let crate::ui::palette::Choice::Command(id) = choice else {
            return None;
        };
        entries
            .iter()
            .find(|entry| entry.command.id == id)
            .map(|entry| entry.intent)
    }

    /// The ground, and on it what has been drawn so far.
    fn draw(&mut self, ui: &mut egui::Ui) {
        crate::ui::sequencer::set_projection(Some(palette::lift));
        crate::ui::sequencer::set_shade(Some(palette::shade));
        let whole = ui.available_rect_before_wrap();
        let painter = ui.painter();
        painter.rect_filled(whole, 0.0, palette::colours().ground);
        let layout = Layout::of(whole);
        // The field is registered to the glass: marks at its corners.
        chassis::marks(painter, layout.field.shrink(4.0), 10.0);
        self.draw_title(painter, layout.title);
        if self.sample.is_some() {
            // The cutting room takes the whole field.
            self.draw_sample(painter, layout.field);
        } else if self.song_view {
            // The field turned over: the song's arrangement in the
            // session's place.
            self.draw_song(painter, layout.field);
        } else {
            self.draw_heads(painter, layout.field);
            // The mixer replaces the scene rows and gives them back: the
            // heads never move across the change, so the eye keeps its place.
            if self.mixing {
                self.draw_mixer(painter, layout.field);
            } else {
                self.draw_lattice(painter, layout.field);
                self.draw_log(painter, layout.field);
            }
        }
        // Over the field: the browser is a window above the work, not a
        // division of it.
        self.draw_browser(painter, layout.field);
        self.draw_help(painter, layout.field);
        self.draw_status(painter, layout.status);
        // One detail region, and the band and the sequencer are two
        // things to put in it. The band wins while it is showing.
        let anchor = if self.chain.is_some() {
            let phase = Phase::of(
                self.transport.motion().is_rolling(),
                self.transport.beat_phase(),
            );
            self.draw_chain(ui.painter(), layout.tray, phase);
            None
        } else {
            self.draw_tray(ui, layout.tray)
        };
        // Over everything in the field: a callout is about one thing, and
        // a callout drawn under anything is a callout pointing through it.
        self.draw_callouts(ui.painter(), whole, anchor);
        // Last of all, because the machine room is not part of the musical
        // surface: it stands in front of the whole of it.
        self.draw_room(ui.painter(), whole);
        crate::ui::nav_cursor::paint(ui.ctx());
        let mut inspector = inspector();
        if inspector.open {
            let panel = egui::Rect::from_min_max(
                egui::pos2(whole.max.x - inspector::WIDTH, whole.min.y),
                whole.max,
            );
            let mut child =
                ui.new_child(egui::UiBuilder::new().max_rect(panel).id_salt("inspector"));
            inspector.ui(&mut child);
        }
    }
}
