//! The kit gallery — every widget in every state, and every real panel
//! rendered against a synthetic `ViewState`, with no engine anywhere.
//!
//! Two jobs, both about making panel work cheap:
//!
//! 1. **Kit changes become visible.** A theme tweak or a new helper shows all
//!    its states side by side instead of hiding in one corner of one panel.
//! 2. **Panels become testable by eye without audio hardware.** Drag the
//!    fake state around — engine off, xruns climbing, a notice firing — and
//!    watch a real panel react. A panel that only looks right with a live
//!    stream is a panel that broke its contract.
//!
//! Actions the previewed panel emits are shown, not performed. That IS the
//! contract: panels return wishes; only the app translates them.
//!
//! Dev-facing, hosted by the `lab` binary. Not shipped in the app's shell.

use crate::ui::action::UiAction;
use crate::ui::device;
use crate::ui::host::{Panel, PanelCx};
use crate::ui::keymap::Keymap;
use crate::ui::kit;
use crate::ui::panels::{arrangement::Arrangement, status::StatusBar, transport::TransportBar};
use crate::ui::theme::Theme;
use crate::ui::tokens::{Density, space};
use crate::ui::vm::ViewState;
use eframe::egui;

/// How many emitted actions to keep on screen.
const ACTION_LOG_LEN: usize = 8;

pub struct Gallery {
    /// The fake world the previewed panel sees.
    vs: ViewState,
    keys: Keymap,
    panels: Vec<Box<dyn Panel>>,
    selected: usize,
    knob: f32,
    meter: f32,
    density: Density,
    log: Vec<String>,
    // Device-widget playground state, all normalized.
    dev_cutoff: f32,
    dev_pan: f32,
    dev_gain: f32,
    dev_mix: f32,
    dev_xy: (f32, f32),
    dev_env: device::Adsr,
    dev_page: usize,
    dev_synth: device::SineSynthUi,
}

impl Default for Gallery {
    fn default() -> Self {
        Self {
            vs: ViewState::demo(),
            keys: Keymap::default(),
            panels: vec![
                Box::new(TransportBar),
                Box::new(StatusBar),
                Box::new(Arrangement),
            ],
            selected: 0,
            knob: 0.62,
            meter: 0.4,
            density: Density::default(),
            log: Vec::new(),
            dev_cutoff: 0.5,
            dev_pan: 0.5,
            dev_gain: 0.9,
            dev_mix: 1.0,
            dev_xy: (0.3, 0.6),
            dev_env: device::Adsr::default(),
            dev_page: 0,
            dev_synth: device::SineSynthUi::default(),
        }
    }
}

impl Gallery {
    /// `theme` is `&mut` because the gallery is where you change the theme
    /// (density, later light mode) and watch everything move at once.
    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &mut Theme) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.theme_controls(ui, theme);
            kit::gap(ui, theme, space::LG);
            self.widgets(ui, theme);
            kit::gap(ui, theme, space::LG);
            self.devices(ui, theme);
            kit::gap(ui, theme, space::LG);
            self.shortcuts(ui, theme);
            kit::gap(ui, theme, space::LG);
            self.panel_preview(ui, theme);
        });
    }

    fn theme_controls(&mut self, ui: &mut egui::Ui, theme: &mut Theme) {
        let current = self.density;
        let picked = kit::section(ui, theme, "theme", |ui| {
            ui.horizontal(|ui| {
                let mut picked = None;
                for density in Density::ALL {
                    if kit::toggle(ui, density == current, density.label()) {
                        picked = Some(density);
                    }
                }
                picked
            })
            .inner
        });
        if let Some(density) = picked {
            self.density = density;
            theme.set_density(density);
        }
    }

    fn widgets(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        kit::section(ui, theme, "text roles", |ui| {
            kit::title(ui, theme, "title");
            kit::label(ui, theme, "label — ordinary body text");
            kit::muted(ui, theme, "muted — secondary, small");
            kit::value(ui, theme, "value 123.45");
            kit::value_state(ui, theme, "value_state ok", true);
            kit::value_state(ui, theme, "value_state bad", false);
            kit::notice(ui, theme, "notice — the user must see this");
        });

        kit::gap(ui, theme, space::SM);
        kit::section(ui, theme, "controls", |ui| {
            ui.horizontal(|ui| {
                kit::button(ui, theme, "button");
                kit::button_hint(ui, theme, "with hint", Some("Ctrl+K"));
                kit::toggle(ui, false, "toggle off");
                kit::toggle(ui, true, "toggle on");
                ui.add_enabled_ui(false, |ui| {
                    kit::button(ui, theme, "disabled");
                });
            });
            kit::gap(ui, theme, space::SM);
            ui.horizontal(|ui| {
                kit::muted(ui, theme, "leds");
                kit::led(ui, theme, true, theme.ok);
                kit::led(ui, theme, true, theme.warn);
                kit::led(ui, theme, true, theme.danger);
                kit::led(ui, theme, false, theme.ok);
            });
        });

        kit::gap(ui, theme, space::SM);
        kit::section(ui, theme, "painted widgets", |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    kit::muted(ui, theme, "knob (drag)");
                    kit::knob(ui, theme, &mut self.knob);
                    kit::value(ui, theme, &format!("{:.2}", self.knob));
                });
                kit::gap(ui, theme, space::LG);
                ui.vertical(|ui| {
                    kit::muted(ui, theme, "meter");
                    ui.add(egui::Slider::new(&mut self.meter, 0.0..=1.0).show_value(false));
                });
                // Every band, so a palette change is judged whole.
                for level in [self.meter, 0.2, 0.6, 0.85, 1.0] {
                    kit::meter(ui, theme, level);
                }
            });
        });

        kit::gap(ui, theme, space::SM);
        kit::section(ui, theme, "structure", |ui| {
            kit::row(ui, theme, "row label", |ui| {
                kit::value(ui, theme, "right-aligned");
            });
            kit::rule(ui, theme);
            kit::row(ui, theme, "another", |ui| {
                kit::toggle(ui, true, "control");
            });
            kit::empty_state(ui, theme, "empty_state — a panel with nothing to say");
        });
    }

    /// The device-widget playground: every `ui::device` widget live, wired
    /// to real `Param`s, inside a real tabbed card — so a theme change or a
    /// widget tweak is judged on the whole family at once.
    fn devices(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let cutoff = device::Param::hz("cutoff", 20.0, 20_000.0).with_default(1_000.0);
        let pan = device::Param::percent("pan").bipolar().with_default(50.0);
        let gain = device::Param::db("gain", -60.0, 6.0).with_default(0.0);
        let mix = device::Param::percent("mix").with_default(100.0);
        let res = device::Param::percent("res");

        kit::section(ui, theme, "device widgets", |ui| {
            ui.horizontal(|ui| {
                device::knob::knob(ui, theme, &cutoff, &mut self.dev_cutoff);
                device::knob::knob(ui, theme, &pan, &mut self.dev_pan);
                kit::gap(ui, theme, space::MD);
                device::fader::fader(ui, theme, &gain, &mut self.dev_gain);
                kit::gap(ui, theme, space::MD);
                device::xy::xy_pad(
                    ui,
                    theme,
                    &cutoff,
                    &res,
                    &mut self.dev_xy.0,
                    &mut self.dev_xy.1,
                );
            });

            kit::gap(ui, theme, space::SM);
            kit::row(ui, theme, "slider + readouts", |ui| {
                device::readout::readout_drag(ui, theme, &mix, &mut self.dev_mix);
                device::fader::slider(ui, theme, &mix, &mut self.dev_mix);
                device::readout::readout(ui, theme, &gain, self.dev_gain);
            });

            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "adsr (drag the handles)");
            device::envelope::adsr(ui, theme, &mut self.dev_env);

            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "spectrum (tracks the cutoff knob)");
            let bins = demo_spectrum(cutoff.value(self.dev_cutoff));
            device::spectrum::spectrum(ui, theme, &bins, DEMO_NYQUIST_HZ);

            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "sine synth — the first real device");
            for edit in device::sine_synth_card(ui, theme, &mut self.dev_synth) {
                self.log.push(format!(
                    "SynthEdit(param {}, {:.2})",
                    edit.param, edit.value
                ));
            }

            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "tabbed card + sections (click the dots)");
            device::tabbed_card(
                ui,
                theme,
                "demo device",
                3,
                &mut self.dev_page,
                |ui, page| {
                    // One shape per page, so the dots visibly do something.
                    let (cols, rows) = [(3, 2), (2, 2), (6, 1)][page % 3];
                    device::sections(ui, theme, cols, rows, |_ui, _i| {});
                },
            );
        });
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        kit::section(ui, theme, "keymap", |ui| {
            for binding in self.keys.bindings() {
                let keys = ui.ctx().format_shortcut(&binding.shortcut);
                kit::row(ui, theme, binding.action.label(), |ui| {
                    kit::value(ui, theme, &keys);
                });
            }
        });
    }

    fn panel_preview(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        kit::section(
            ui,
            theme,
            "panel preview — real panels, fake world",
            |ui| {
                ui.horizontal(|ui| {
                    for i in 0..self.panels.len() {
                        let title = self.panels[i].title();
                        if kit::toggle(ui, i == self.selected, title) {
                            self.selected = i;
                        }
                    }
                });
                kit::gap(ui, theme, space::SM);

                ui.horizontal(|ui| {
                    if kit::toggle(ui, self.vs.engine_running, "engine running") {
                        self.vs.engine_running = !self.vs.engine_running;
                    }
                    if kit::toggle(ui, self.vs.playing, "playing") {
                        self.vs.playing = !self.vs.playing;
                    }
                    if kit::toggle(ui, self.vs.xruns > 0, "xruns") {
                        self.vs.xruns = u64::from(self.vs.xruns == 0);
                    }
                    if kit::toggle(ui, self.vs.notice.is_some(), "notice") {
                        self.vs.notice = match self.vs.notice {
                            Some(_) => None,
                            None => Some("audio stream dead — no blocks for 1.4s".to_owned()),
                        };
                    }
                });

                kit::gap(ui, theme, space::SM);
                let mut actions = Vec::new();
                egui::Frame::new()
                    .fill(theme.surface)
                    .inner_margin(egui::Margin::same(theme.sp(space::SM) as i8))
                    .show(ui, |ui| {
                        let mut cx = PanelCx::new(theme, &self.vs, &self.keys, &mut actions);
                        self.panels[self.selected].show(ui, &mut cx);
                    });

                for action in actions {
                    self.log.push(describe(action));
                }
                if self.log.len() > ACTION_LOG_LEN {
                    let drop = self.log.len() - ACTION_LOG_LEN;
                    self.log.drain(..drop);
                }

                kit::gap(ui, theme, space::SM);
                kit::muted(ui, theme, "actions emitted (shown, never performed)");
                if self.log.is_empty() {
                    kit::muted(ui, theme, "—");
                }
                for line in &self.log {
                    kit::value(ui, theme, line);
                }
            },
        );
    }
}

/// Nyquist of the fake analyzer feeding the spectrum demo.
const DEMO_NYQUIST_HZ: f32 = 24_000.0;
/// Bins in the fake spectrum.
const DEMO_BINS: usize = 128;

/// A plausible spectrum: a gentle pink-ish slope plus a resonant bump at
/// `peak_hz`, so turning the cutoff knob visibly moves the trace.
fn demo_spectrum(peak_hz: f32) -> Vec<f32> {
    (0..DEMO_BINS)
        .map(|i| {
            let f = (i as f32 + 0.5) / DEMO_BINS as f32 * DEMO_NYQUIST_HZ;
            let slope = 0.55 * (1.0 - (f / DEMO_NYQUIST_HZ).sqrt());
            let bump = 0.4 * (-(f / peak_hz.max(1.0)).ln().powi(2) * 2.0).exp();
            (slope + bump).clamp(0.0, 1.0)
        })
        .collect()
}

fn describe(action: UiAction) -> String {
    match action {
        UiAction::SetTempo(bpm) => format!("SetTempo({bpm:.1})"),
        UiAction::SetDensity(d) => format!("SetDensity({})", d.label()),
        UiAction::TogglePanel(id) => format!("TogglePanel({id})"),
        UiAction::FocusPanel(id) => format!("FocusPanel({id})"),
        other => format!("{other:?}"),
    }
}
