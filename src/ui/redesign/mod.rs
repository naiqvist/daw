//! The replacement application frame.
//!
//! This layer is intentionally separate from the legacy dock and its panels.
//! It owns only persistent view state and rendering; application data and
//! engine commands continue to cross the boundary through explicit inputs.

pub mod arrangement;
pub mod browser;
pub mod chain;
mod focus;
mod grammar;
mod grid_resolution;
mod help;
pub mod keyboard;
mod layout_grid;
pub mod lens;
pub mod midi_typing;
#[cfg(test)]
mod principles;
mod registers;
mod roll;
pub mod sequence;
mod sequence_grid;
mod signs;
pub mod tools;
pub mod transport;
mod trig_info;
mod verbs;

use eframe::egui;

pub(crate) const OUTLINE: egui::Color32 = egui::Color32::WHITE;
/// The transport and browser read as one continuous L-shaped surface.
pub(crate) const SURFACE_FRAME: egui::Color32 = egui::Color32::from_gray(10);
/// The lower editor is quieter than the framing controls around the canvas.
pub(crate) const SURFACE_SEQUENCE: egui::Color32 = egui::Color32::from_gray(7);
/// Utility controls sit one tonal step above the main framing surface.
pub(crate) const SURFACE_UTILITY: egui::Color32 = egui::Color32::from_gray(16);

/// Fixed height of the shared detail strip, in points — a constant, never
/// a window fraction. The hand may drag the seam; egui remembers.
const DETAIL_H: f32 = 260.0;
/// How tall the hand may drag the strip. Also fixed.
const DETAIL_MAX_H: f32 = 480.0;

/// The detail strip's occupant: the selected track's time-detail (the
/// sequencer) or its sound-detail (the device chain). One address, one
/// occupant — the performer is never in both at once, and the panel not
/// under the hands is steady state, which earns no pixels. The occupant
/// is whichever detail view held focus last, so Tab is also the flip.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Detail {
    #[default]
    Sequence,
    Chain,
}

/// The redesign's persistent, UI-local state.
#[derive(Default)]
pub struct Redesign {
    arrangement: arrangement::ArrangementPanel,
    browser: browser::BrowserPanel,
    chain: chain::ChainPanel,
    detail: Detail,
    help: bool,
    keyboard: keyboard::Keyboard,
    midi_typing: midi_typing::MidiTyping,
    sequence: sequence::SequencePanel,
    transport: transport::TransportBar,
    /// A pitch played on HARDWARE, waiting for the next frame to enter it.
    ///
    /// Hardware notes join the same road the typed piano already uses, so
    /// entry has one meaning wherever the note came from. Held here rather
    /// than threaded through `show`'s argument list, which is already at
    /// its limit.
    queued_pitch: Option<crate::pitch::Pitch>,
}

pub struct Outcome {
    pub transport: transport::Outcome,
    pub browser: browser::Outcome,
    pub chain: chain::Outcome,
    pub sequence: sequence::Outcome,
    pub arrangement_focused: bool,
}

impl Redesign {
    pub fn focus_arrangement(&mut self) {
        self.keyboard.focus(keyboard::FocusTarget::Arrangement);
    }

    /// Enter a pitch played on hardware. The newest wins: a frame can
    /// only enter one note, and the most recent key is the one the hand
    /// meant.
    pub fn enter_pitch(&mut self, pitch: crate::pitch::Pitch) {
        self.queued_pitch = Some(pitch);
    }

    pub fn focus_sequence(&mut self) {
        self.detail = Detail::Sequence;
        self.keyboard.focus(keyboard::FocusTarget::Sequence);
    }

    /// The SONG arrangement, drawn into the frame's center when the app
    /// swaps the center to the new world (C1, the song-bridge brief).
    /// Constructed here because the grammar's Voice is module-private.
    pub fn show_arrangement(
        &mut self,
        ui: &mut egui::Ui,
        view: arrangement::View<'_>,
    ) -> arrangement::Outcome {
        let focused = self.keyboard.current() == keyboard::FocusTarget::Arrangement;
        let outcome = self.arrangement.show(
            ui,
            focused,
            &mut grammar::Voice {
                sentence: &mut self.keyboard.sentence,
                registers: &mut self.keyboard.registers,
            },
            view,
        );
        if outcome.claim_focus {
            self.keyboard.focus(keyboard::FocusTarget::Arrangement);
        }
        if outcome.open_pattern {
            self.focus_sequence();
        }
        outcome
    }

    /// The song pattern under the arrangement cursor, for the app to
    /// project into the sequence view.
    pub fn selected_song_pattern(
        &mut self,
        song: &crate::sequencing::Song,
    ) -> Option<crate::sequencing::PatternId> {
        self.arrangement.selected_pattern(song)
    }

    /// The song track under the arrangement cursor — whose authority
    /// decides the pitch language entry speaks.
    pub fn selected_song_track(&mut self, song: &crate::sequencing::Song) -> Option<usize> {
        self.arrangement.selected_track(song)
    }

    /// The arrangement selection's beat span, for loop-the-selection.
    pub fn song_selection_beats(&mut self, song: &crate::sequencing::Song) -> (f32, f32) {
        self.arrangement.selection_range_beats(song)
    }

    /// Add an instrument track to the song, cursor landing on it.
    pub fn song_add_track(&mut self, song: &mut crate::sequencing::Song) {
        self.arrangement.add_track(song);
    }

    /// Draw the application frame. New regions join here, keeping the app
    /// layer a composition point instead of a second UI implementation.
    #[allow(clippy::too_many_arguments)] // One read-only view per composed surface.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        transport_view: transport::View<'_>,
        browser_view: browser::View<'_>,
        chain_view: &chain::View,
        sequence_view: Option<sequence::ClipView<'_>>,
        entry_mode: midi_typing::EntryMode,
        lens_view: &lens::LensView,
        playhead_beats: f64,
        browser_visible: bool,
        sequence_visible: bool,
    ) -> Outcome {
        // The reference card: ? summons it, ? or Escape dismisses it.
        // Consumed before the grammar so the card's Escape never doubles
        // as a sentence abandon. Text fields keep their question marks.
        if !ui.ctx().egui_wants_keyboard_input() {
            let questioned = ui.ctx().input_mut(|input| {
                input.consume_key(egui::Modifiers::SHIFT, egui::Key::Questionmark)
                    || input.consume_key(egui::Modifiers::NONE, egui::Key::Questionmark)
            });
            if questioned {
                self.help = !self.help;
            } else if self.help
                && ui
                    .ctx()
                    .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                self.help = false;
            }
        }

        let queued_pitch = self.queued_pitch.take();
        let detail_target = match self.detail {
            Detail::Sequence => keyboard::FocusTarget::Sequence,
            Detail::Chain => keyboard::FocusTarget::Chain,
        };
        let nav = self.keyboard.update(ui.ctx(), detail_target);
        if nav.flip_detail {
            self.detail = match self.detail {
                Detail::Sequence => Detail::Chain,
                Detail::Chain => Detail::Sequence,
            };
            self.keyboard.focus(match self.detail {
                Detail::Sequence => keyboard::FocusTarget::Sequence,
                Detail::Chain => keyboard::FocusTarget::Chain,
            });
        }
        let focus = self.keyboard.current();
        let midi = self.midi_typing.update(ui.ctx(), entry_mode);
        let transport = self.transport.show(
            ui,
            focus == keyboard::FocusTarget::Transport,
            transport_view,
            midi.status,
        );
        // Focus landing on a detail view makes it the strip's occupant;
        // focus leaving for another panel changes nothing — the strip
        // keeps showing what the performer last worked in.
        if focus == keyboard::FocusTarget::Sequence {
            self.detail = Detail::Sequence;
        } else if focus == keyboard::FocusTarget::Chain {
            self.detail = Detail::Chain;
        }
        let occupant = if sequence_visible {
            self.detail
        } else {
            Detail::Chain
        };

        let mut sequence = sequence::Outcome::default();
        let mut chain = chain::Outcome::default();
        egui::Panel::bottom("redesign-detail")
            .resizable(true)
            .show_separator_line(false)
            .default_size(DETAIL_H)
            .max_size(DETAIL_MAX_H)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE_SEQUENCE)
                    .corner_radius(crate::ui::tokens::radius::PANEL)
                    .stroke(egui::Stroke::NONE),
            )
            .show(ui, |ui| match occupant {
                Detail::Sequence => {
                    sequence = self.sequence.show(
                        ui,
                        focus == keyboard::FocusTarget::Sequence,
                        grammar::Voice {
                            sentence: &mut self.keyboard.sentence,
                            registers: &mut self.keyboard.registers,
                        },
                        // A typed note wins over a hardware one only
                        // because it is the more deliberate of the two;
                        // either way exactly one note enters per frame.
                        midi.entered
                            .map(|entered| match entered {
                                midi_typing::Entered::Midi(midi) => {
                                    crate::pitch::Pitch::from_midi(midi)
                                }
                                midi_typing::Entered::Degree { degree, period } => {
                                    crate::pitch::Pitch::degree(degree, period)
                                }
                            })
                            .or(queued_pitch),
                        sequence_view,
                        lens_view,
                    );
                }
                Detail::Chain => {
                    chain = self.chain.show(
                        ui,
                        focus == keyboard::FocusTarget::Chain,
                        &mut grammar::Voice {
                            sentence: &mut self.keyboard.sentence,
                            registers: &mut self.keyboard.registers,
                        },
                        chain_view,
                    );
                }
            });
        if sequence.claim_focus {
            self.keyboard.focus(keyboard::FocusTarget::Sequence);
        }
        if chain.claim_focus {
            self.keyboard.focus(keyboard::FocusTarget::Chain);
        }
        let browser = if browser_visible {
            self.browser.show(
                ui,
                focus == keyboard::FocusTarget::Browser,
                &mut self.keyboard.sentence,
                browser_view,
            )
        } else {
            browser::Outcome::default()
        };
        if browser.claim_focus {
            self.keyboard.focus(keyboard::FocusTarget::Browser);
        }
        tools::show(
            ui,
            focus == keyboard::FocusTarget::Tools,
            &self.keyboard.registers,
            &self.keyboard.sentence,
        );
        help::show(ui.ctx(), &mut self.help);
        let _ = playhead_beats;
        Outcome {
            transport,
            browser,
            chain,
            sequence,
            arrangement_focused: focus == keyboard::FocusTarget::Arrangement,
        }
    }
}
