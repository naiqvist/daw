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
mod brick_card;
mod browser;
mod callouts;
mod chassis;
mod composer;
mod deck;
mod desk;
mod face;
mod faces;
mod forge;
mod heads;
mod help;
mod input;
mod inspector;
mod keyicon;
mod kit_card;
mod lab;
mod lattice;
mod log;
mod matrix;
mod meter;
#[cfg(test)]
mod meter_perf_tests;
mod midi_lab;
mod midi_ladder;
mod mixer;
mod modulation;
mod palette;
mod quad_card;
#[cfg(test)]
mod rom_tests;
mod room;
mod sample;
mod sampler_card;
mod scomp_card;
#[cfg(test)]
mod slice_tests;
mod song;
mod stab_card;
pub(in crate::ui::stage) mod status;
mod strip;
#[cfg(feature = "visuals")]
mod visuals;
#[cfg(test)]
mod parameter_tests;
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
use crate::ui::stage::RefusalReason;
use crate::ui::stage::grid::Step;
use eframe::egui;

/// An axis nothing is drawn along yet can hold everything: no offset
/// needs to move to keep the cursor in sight.
const HOLDS_EVERYTHING: usize = usize::MAX;

/// Typed statements exposed by the command centre. The palette owns
/// discovery and text entry; parsing and mutation stay in the stage core.
/// Two speak to the timeline at the cursor's tick; `lane` and `sound`
/// speak to the cursor's track; `swing` and `scale` to the open clip.
const TIMELINE_COMMANDS: [crate::ui::palette::TypedCommand; 8] = [
    crate::ui::palette::TypedCommand {
        name: "tempo",
        usage: "tempo <bpm>  |  tempo clear",
    },
    crate::ui::palette::TypedCommand {
        name: "meter",
        usage: "meter (tracker)  |  meter <N>/<D>  |  meter clear",
    },
    crate::ui::palette::TypedCommand {
        name: "lane",
        usage: "lane drum  |  lane clear",
    },
    crate::ui::palette::TypedCommand {
        name: "midi",
        usage: "midi <clip tag>  |  midi a1",
    },
    crate::ui::palette::TypedCommand {
        name: "lab",
        usage: "lab",
    },
    crate::ui::palette::TypedCommand {
        name: "sound",
        usage: "sound save <name>  |  sound load <name>  |  sound rename <old> <new>",
    },
    crate::ui::palette::TypedCommand {
        name: "swing",
        usage: "swing <50..80>  |  swing clear",
    },
    crate::ui::palette::TypedCommand {
        name: "scale",
        usage: "scale 1/8 1/4 1/2 3/4 1 3/2 2 4 8  |  scale clear",
    },
];

/// The window carved: a title row, the field, a status strip.
struct Layout {
    title: egui::Rect,
    deck: egui::Rect,
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
        let deck = egui::Rect::from_min_max(
            title.left_bottom(),
            egui::pos2(whole.max.x, title.max.y + deck::deck_h()),
        );
        let status =
            egui::Rect::from_min_max(egui::pos2(whole.min.x, whole.max.y - status_h), whole.max);
        let band = egui::Rect::from_min_max(deck.left_bottom(), status.right_top());
        // The tray is cut off the FOOT of the band, fixed: the session
        // above keeps its shape whether or not the tray has a clip.
        let tray_top = (band.max.y - tray::tray_h()).max(band.min.y);
        let field = egui::Rect::from_min_max(band.min, egui::pos2(band.max.x, tray_top));
        let tray = egui::Rect::from_min_max(egui::pos2(band.min.x, tray_top), band.max);
        Self {
            title,
            deck,
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
        if self.poll_kiln() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        if self.poll_library() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(50));
        }
        // A utility room is a true modal: it gets the frame's keyboard
        // before the musical surface and may close itself with Escape.
        self.update_utility(ui.ctx());
        // The console's colours follow the ground: the light file reads
        // while the ground is turned over.
        palette::set_polarity(self.polarity);
        // And so does the glass. The whole-field treatment is an
        // emissive idea; on paper there is no tube to simulate, so the
        // shell hands the light ground the frame as it was authored.
        crate::shell::post::set_ground_light(
            ui.ctx(),
            self.polarity == crate::design::Polarity::Light,
        );
        let utility_open = self.utility.is_open();

        // Ctrl+Shift+P also works from a focused field. Bare `:` remains
        // ordinary text there. Neither shortcut resets an open query.
        if !utility_open
            && !self.palette.is_open()
            && (ui.input_mut(|input| {
                input.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::P)
            }) || (!ui.ctx().egui_wants_keyboard_input()
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Colon))))
        {
            self.palette.open();
        }
        let palette_owned_frame = self.palette.is_open();
        if !utility_open && let Some(intent) = self.pump_palette(ui.ctx()) {
            let _ = self.apply(intent);
        }
        let utility_open = self.utility.is_open();
        // While the palette is open it owns the keyboard OUTRIGHT.
        let palette_open = palette_owned_frame || utility_open;

        self.hold_browser_for_exit();

        let collect_text = self.collects_text();
        let grammar_owns_escape = self.grammar_owns_escape();
        let scope = self.scope_context();
        let selection_scope = self.selection_scope();
        let selection_held = selection_scope && ui.input(|input| input.key_down(egui::Key::X));
        // X is a held gesture only on Root/Song, but it is also a one-shot
        // command in modal scopes (the p-lock editor's INCLUDE toggle).
        // Observe the physical, non-repeating press everywhere so those
        // scopes receive X exactly once while a held surface gesture can
        // still extend with the arrows.
        let selection_pressed = ui.input(|input| {
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
        let inputs = if palette_open || (self.lab.open && ui.ctx().egui_wants_keyboard_input()) {
            Vec::new()
        } else {
            ui.input_mut(|input| {
                let chords = input::consume_chords(input, scope, |modifiers, key| {
                    (grammar_owns_escape && modifiers == Mods::NONE && key == Key::Escape)
                    // TYPING OUTRANKS THE BINDING. While a field is
                    // taking text, a bare character key is that
                    // character. Without this the chord layer took the
                    // key first and the text was dropped to keep the
                    // keystroke from counting twice — which is how a
                    // space typed into the browser's find field started
                    // the transport instead of reaching the query, and
                    // how a question mark opened the codebook rather
                    // than being searched for.
                    || (collect_text && modifiers == Mods::NONE && key.types_a_character())
                    || matches!(keymap::dispatch(scope,keymap::StageInput::Chord(modifiers,key)),Some(StageIntent::HeroTool(verb)) if !self.deck_hero_tools().iter().any(|t|t.verb==verb))
                });
                let mut stage_inputs: Vec<keymap::StageInput> = chords
                    .iter()
                    .map(|(modifiers, key)| keymap::StageInput::Chord(*modifiers, *key))
                    .collect();
                if collect_text {
                    for event in &input.events {
                        let egui::Event::Text(text) = event else {
                            continue;
                        };
                        // No keystroke counts twice: the chord layer
                        // above left every character key in the input
                        // rather than taking it.
                        stage_inputs.extend(text.chars().map(keymap::StageInput::Text));
                    }
                }
                stage_inputs
            })
        };
        let bound: Vec<keymap::StageInput> = inputs.clone();
        self.take_inputs(inputs, selection_held, selection_pressed);
        let step_mask = if palette_open {
            self.steps.map_or(0, |steps| steps.held)
        } else {
            ui.input(input::step_mask)
        };
        // Real elapsed time, not the predicted frame: the view repaints
        // slowly at rest, and a hold is measured against the clock.
        let frame_dt = ui.input(|input| input.unstable_dt.min(1.0));
        self.steps_frame_timed(step_mask, frame_dt);

        // Inside a clip, the letters may be pitches. Read after the
        // stage's own chords, so `^T` is never read as a T.
        let update = (!palette_open)
            .then(|| self.pitch_entry_mode())
            .flatten()
            .map(|mode| self.midi_typing.update(ui.ctx(), mode));
        let enter_held = ui.input(|input| input.key_down(egui::Key::Enter));
        self.take_pitch_entry(update, enter_held);

        let layout = Layout::of(ui.available_rect_before_wrap());
        let field = layout.field;
        self.follow_cursor(
            if self.mixing {
                mixer::track_capacity(field.width())
            } else {
                heads::capacity(field.width())
            },
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
                self.inside.map(|o| (self.song.tag_of(o.pattern), o.track)),
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
            // What the last open dropped, one line each, once.
            for line in self.take_migration_log() {
                telemetry().dropped(line);
            }
        }
        // The refusal is drawn where it happened: the wall the cursor
        // pressed against lights on the mark itself, not only named on
        // the strip. A step that had nowhere to go lights that side; a
        // refusal with no direction to it lights the whole frame.
        if let Some(refusal) = self.refusal {
            use crate::ui::nav_cursor::Wall;
            let wall = match refusal.reason {
                RefusalReason::Edge(Step::Up) => Wall::Up,
                RefusalReason::Edge(Step::Down) => Wall::Down,
                RefusalReason::Edge(Step::Left) => Wall::Left,
                RefusalReason::Edge(Step::Right) => Wall::Right,
                RefusalReason::AtTop => Wall::Left,
                _ => Wall::All,
            };
            crate::ui::nav_cursor::refuse(ui.ctx(), wall, palette::colours().alert);
        }
        if self.wants_repaint() {
            ui.ctx().request_repaint();
        }

        self.draw(ui);
        #[cfg(feature = "visuals")]
        if self.visual_preview.is_some() || self.visual_export.is_some() {
            self.show_visuals(ui.ctx());
        }
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
        let mut typed = TIMELINE_COMMANDS.to_vec();
        typed.extend_from_slice(super::phrase::COMMANDS);
        typed.extend_from_slice(super::parameter_command::COMMANDS);
        typed.extend_from_slice(super::spectral_command::COMMANDS);
        #[cfg(feature = "visuals")]
        typed.extend_from_slice(super::visual_command::COMMANDS);
        typed.extend_from_slice(super::utility::COMMANDS);
        typed.extend_from_slice(super::navigation_command::COMMANDS);
        match self
            .palette
            .show(ctx, &theme, &commands, &typed)?
        {
            crate::ui::palette::Choice::Command(id) => entries
                .iter()
                .find(|entry| entry.command.id == id)
                .map(|entry| entry.intent),
            crate::ui::palette::Choice::Typed(line) => {
                let _ = self.apply_timeline_command(&line);
                None
            }
        }
    }

    /// The ground, and on it what has been drawn so far.
    fn draw(&mut self, ui: &mut egui::Ui) {
        crate::ui::sequencer::set_projection(Some(palette::lift));
        crate::ui::sequencer::set_shade(Some(palette::shade));
        let whole = ui.available_rect_before_wrap();
        let painter = ui.painter().clone();
        painter.rect_filled(whole, 0.0, palette::colours().ground);
        let layout = Layout::of(whole);
        let modulation_room = egui::Rect::from_min_max(layout.field.min, layout.tray.max);
        // The lab is a full-screen mode: one room from the title to the
        // status line, with no deck strip and no tray under it.
        let lab_room = egui::Rect::from_min_max(layout.deck.min, layout.status.right_top());
        // The meter takes the same room the lab does, and for the same
        // reason: its subject is the whole song across the whole bar, and
        // a page row it cannot answer would be a row of dead keys.
        let meter_room = lab_room;
        // The field is registered to the glass: marks at its corners.
        chassis::marks(
            &painter,
            if self.modulation.is_some() {
                modulation_room.shrink(4.0)
            } else if self.lab.open {
                lab_room.shrink(4.0)
            } else if self.meter_open() {
                meter_room.shrink(4.0)
            } else {
                layout.field.shrink(4.0)
            },
            10.0,
        );
        self.draw_title(&painter, layout.title);
        if !self.lab.open && !self.meter_open() {
            self.draw_deck(&painter, layout.deck);
        }
        let anchor = if self.modulation.is_some() {
            // A project-wide patchbay needs both the field and its detail
            // band. The session remains exactly where it was underneath and
            // comes back with its cursor intact when the workspace closes.
            self.draw_modulation(ui, modulation_room);
            self.draw_help(&painter, modulation_room);
            None
        } else if self.lab.open {
            self.draw_lab(ui, &painter, lab_room);
            self.draw_help(&painter, lab_room);
            None
        } else if self.meter_open() {
            self.draw_meter(&painter, meter_room);
            self.draw_help(&painter, meter_room);
            None
        } else {
            if self.sample.is_some() {
                // The cutting room takes the whole field.
                self.draw_sample(&painter, layout.field);
            } else if self.forge.is_some() {
                // So does the forge.
                self.draw_forge(&painter, layout.field);
            } else if self.song_view {
                // The field turned over: the song's arrangement in the
                // session's place.
                self.draw_song(&painter, layout.field);
            } else {
                self.draw_heads(&painter, layout.field);
                // The mixer replaces the scene rows and gives them back: the
                // heads never move across the change, so the eye keeps its place.
                if self.mixing {
                    self.draw_mixer(&painter, layout.field);
                } else {
                    self.draw_lattice(&painter, layout.field);
                    self.draw_desk(&painter, layout.field);
                    self.draw_log(&painter, layout.field);
                }
            }
            // Over the field: the deck's window, then the browser above
            // it — both are windows above the work, not divisions of it.
            let compact_material = self.deck_open()
                && self.deck_hero_height() == crate::pages::HeroHeight::Tall
                && layout.field.height() < 360.0;
            let deck_field = if compact_material {
                egui::Rect::from_min_max(layout.field.min, layout.tray.max)
            } else {
                layout.field
            };
            self.interact_deck_hero(ui, deck_field);
            self.draw_deck_window(&painter, deck_field);
            self.draw_matrix_window(&painter, layout.field);
            self.draw_browser(&painter, deck_field);
            self.draw_help(&painter, deck_field);
            // One detail region, and the band and the sequencer are two
            // things to put in it. The band wins while it is showing.
            if compact_material {
                None
            } else if self.chain.is_some() {
                let phase = Phase::of(
                    self.transport.motion().is_rolling(),
                    self.transport.beat_phase(),
                );
                self.draw_chain(ui, layout.tray, phase);
                None
            } else {
                self.draw_tray(ui, layout.tray)
            }
        };
        self.draw_status(&painter, layout.status);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::stage::keymap::ScopeContext;

    /// One actual frame, including the event-consumption layer that a direct
    /// core `handle_key` test intentionally bypasses.
    fn frame_key(stage: &mut Stage, ctx: &egui::Context, key: egui::Key, repeat: bool) {
        frame_chord(stage, ctx, egui::Modifiers::NONE, key, repeat);
    }

    fn frame_chord(
        stage: &mut Stage,
        ctx: &egui::Context,
        modifiers: egui::Modifiers,
        key: egui::Key,
        repeat: bool,
    ) {
        let event = egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat,
            modifiers,
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1_280.0, 800.0),
                )),
                // Leave the key down between frames: an OS repeat is a
                // second press event on that same held key, not a release and
                // a fresh press.
                events: vec![event],
                ..Default::default()
            },
            |ui| stage.show(ui),
        );
        output.textures_delta.clear();
    }

    #[test]
    fn consecutive_palette_sentences_work_at_one_event_per_frame() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        crate::ui::stage::tests::into_clip(&mut stage);
        let ctx = egui::Context::default();
        crate::install_stage_fonts(&ctx);
        let id = stage.inside.unwrap().pattern;
        let chord = |key, modifiers| vec![
            egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers },
            egui::Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers },
        ];
        let commands = ["rhythm 64 0,24 gate 8 vel 112", "rhythm 64 40,46 gate 8 vel 84"];
        // A previous editor may still own egui focus when a clip opens.
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("previous_field")));
        for command in commands {
            for events in [
                chord(egui::Key::P, egui::Modifiers::CTRL | egui::Modifiers::SHIFT),
                vec![egui::Event::Text(command.into())],
                chord(egui::Key::Enter, egui::Modifiers::NONE),
            ] {
                let mut output = ctx.run_ui(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                    events, ..Default::default()
                }, |ui| stage.show(ui));
                output.textures_delta.clear();
            }
            assert!(!stage.palette.is_open(), "{command}");
            assert!(stage.notice.as_deref().unwrap_or_default().contains("onsets edited"), "{:?}", stage.notice);
        }
        let pattern = stage.song.pattern(id).unwrap();
        let notes: Vec<_> = (0..pattern.step_count()).flat_map(|step| &pattern.trig(step).notes).collect();
        assert_eq!(notes.len(), 4);
        assert!(notes.iter().all(|n| n.length_ticks == 24));
        assert_eq!(notes.iter().map(|n| n.velocity).collect::<Vec<_>>(), vec![112, 112, 84, 84]);
    }

    #[test]
    fn an_actual_plock_frame_accepts_x_once_and_ignores_repeat() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        crate::ui::stage::tests::into_clip(&mut stage);
        assert_eq!(stage.apply(StageIntent::PlockEditor), ApplyOutcome::Changed);
        stage
            .plock_editor
            .as_mut()
            .expect("lock editor")
            .param_cursor = 1;
        let ctx = egui::Context::default();
        crate::install_stage_fonts(&ctx);

        frame_key(&mut stage, &ctx, egui::Key::X, false);
        let after_press = stage.plock_editor.clone();
        assert!(
            after_press
                .as_ref()
                .expect("lock editor")
                .selected_params
                .contains(&1)
        );

        frame_key(&mut stage, &ctx, egui::Key::X, true);
        assert_eq!(stage.plock_editor, after_press);
    }

    #[test]
    fn an_actual_modulation_frame_opens_adds_and_patches_without_a_mouse() {
        let mut stage = Stage::new();
        stage.set_palette_open(false);
        let ctx = egui::Context::default();
        crate::install_stage_fonts(&ctx);

        frame_chord(
            &mut stage,
            &ctx,
            egui::Modifiers {
                ctrl: true,
                command: true,
                shift: true,
                ..Default::default()
            },
            egui::Key::M,
            false,
        );
        assert_eq!(stage.scope_context(), ScopeContext::Modulation);

        frame_key(&mut stage, &ctx, egui::Key::L, false);
        assert_eq!(stage.song.modulators.len(), 1);

        frame_key(&mut stage, &ctx, egui::Key::Tab, false);
        assert_eq!(
            stage.modulation.as_ref().expect("panel").focus,
            crate::ui::stage::modulation::Focus::Targets
        );
        frame_key(&mut stage, &ctx, egui::Key::X, false);
        assert_eq!(stage.song.mod_wires.len(), 1);
    }
}
