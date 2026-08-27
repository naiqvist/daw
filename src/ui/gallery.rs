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

use crate::params;
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
    /// The segmented switch's normalized value — a discrete param is a
    /// normalized value like any other, which is the point.
    dev_mode: f32,
    /// Phase of the meter demo's synthetic signal, in seconds. A meter is
    /// the one widget you cannot judge from a still frame — its whole
    /// character is how it MOVES — so the gallery feeds it something
    /// alive rather than a fixed level.
    dev_meter_t: f32,
    /// The value fields' normalized values.
    dev_field_hz: f32,
    dev_field_ms: f32,
    dev_field_db: f32,
    /// The filter curve's state, and the two switches driving it.
    dev_filter: device::filter::Filter,
    dev_filter_mode: f32,
    dev_filter_slope: f32,
    dev_filter_drive: f32,
    /// The compressor demo's settings, and the knob positions driving it.
    dev_dyn: device::dynamics::Dynamics,
    dev_dyn_mode: f32,
    dev_dyn_threshold: f32,
    dev_dyn_ratio: f32,
    dev_dyn_knee: f32,
    dev_dyn_attack: f32,
    dev_dyn_release: f32,
    /// The waveshaper demo.
    dev_shaper: device::shaper::Shaper,
    dev_shaper_mode: f32,
    dev_shaper_drive: f32,
    dev_shaper_bias: f32,
    dev_shaper_mix: f32,
    dev_sync: f32,
    dev_pan: f32,
    dev_gain: f32,
    dev_mix: f32,
    dev_xy: (f32, f32),
    dev_env: device::Adsr,
    dev_page: usize,
    dev_synth: device::SineSynthUi,
    dev_poly: device::PolyUi,
    dev_lofi: device::LofiUi,
    dev_sheen: device::SheenUi,
    dev_disperser: device::DisperserUi,
    dev_tilt: device::TiltUi,
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
            dev_mode: 0.0,
            dev_meter_t: 0.0,
            dev_field_hz: 0.5,
            dev_field_ms: 0.4,
            dev_field_db: 0.8,
            // A resonant corner rather than a flat one: the gallery is
            // where someone looks to see what the widget DOES.
            dev_filter: device::filter::Filter {
                cutoff_hz: 900.0,
                q: 6.0,
                ..device::filter::Filter::default()
            },
            dev_filter_mode: 0.0,
            dev_filter_slope: 0.6,
            dev_filter_drive: 0.0,
            dev_dyn: device::dynamics::Dynamics::default(),
            dev_dyn_mode: 0.0,
            dev_dyn_threshold: 0.7,
            dev_dyn_ratio: 0.35,
            dev_dyn_knee: 0.25,
            dev_dyn_attack: 0.35,
            dev_dyn_release: 0.5,
            dev_shaper: device::shaper::Shaper::default(),
            dev_shaper_mode: 0.25,
            dev_shaper_drive: 0.45,
            dev_shaper_bias: 0.5,
            dev_shaper_mix: 1.0,
            dev_sync: 1.0,
            dev_pan: 0.5,
            dev_gain: 0.9,
            dev_mix: 1.0,
            dev_xy: (0.3, 0.6),
            dev_env: device::Adsr::default(),
            dev_page: 0,
            dev_synth: device::SineSynthUi::default(),
            dev_poly: device::PolyUi::default(),
            dev_lofi: device::LofiUi::default(),
            dev_sheen: device::SheenUi::default(),
            dev_disperser: device::DisperserUi::default(),
            // NOT the device's default, which is flat — deliberately, see
            // `params::tilt`. A preview of a corrective device sitting at
            // its neutral setting is a preview of a straight line, so the
            // gallery leans this one over to show what the card draws.
            dev_tilt: device::TiltUi {
                tilt: device::tilt_norm(params::tilt::TILT, 6.0),
                ..device::TiltUi::default()
            },
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
            self.formations(ui, theme);
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
    /// Well formations, side by side: what the card layout can be asked
    /// for, with nothing in the wells to distract from the shape.
    ///
    /// Deliberately NOT drawn inside `device::card`. A card clamps its
    /// locked height to whatever the region offers, and a scrolling
    /// gallery section does not reserve it — every card in one renders as
    /// a squashed strip with its wells spilling out the bottom. That is a
    /// gallery problem worth fixing on its own; a diagram of layouts
    /// should not be the thing that waits for it.
    fn formations(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        // A modest stand-in for a control, so the shapes stay compact.
        let leaf = device::Footprint::new(30.0, 26.0);
        let samples: [(&str, device::Wells); 6] = [
            (
                "row of 3",
                device::Wells::new().row([device::Well::divided(3, 1).each(leaf, theme)]),
            ),
            (
                "2 x 2",
                device::Wells::new().row([device::Well::divided(2, 2).each(leaf, theme)]),
            ),
            (
                "3 over 4",
                device::Wells::new().row([device::Well::rows_of([3, 4]).each(leaf, theme)]),
            ),
            (
                "1 over 3",
                device::Wells::new().row([device::Well::rows_of([1, 3]).each(leaf, theme)]),
            ),
            (
                "2, 3, 4",
                device::Wells::new().row([device::Well::rows_of([2, 3, 4]).each(leaf, theme)]),
            ),
            (
                "well + 3 over 4",
                device::Wells::new().row([
                    device::Well::one().fits(leaf),
                    device::Well::rows_of([3, 4]).each(leaf, theme),
                ]),
            ),
        ];

        kit::section(ui, theme, "well formations", |ui| {
            // Explicit rows of two rather than `horizontal_wrapped`.
            // Each sample is its own vertical (caption over shape), and a
            // wrapped layout measures the cursor rather than the child it
            // is about to add — so the last samples marched off the right
            // edge instead of wrapping.
            for chunk in samples.chunks(2) {
                ui.horizontal(|ui| {
                    for (name, spec) in chunk {
                        ui.vertical(|ui| {
                            kit::muted(ui, theme, name);
                            // Each sample gets EXACTLY the size its own
                            // contract asks for — which is also the proof
                            // that the contract is right, since anything
                            // clipped here was under-reserved.
                            let size = egui::vec2(spec.min_width(theme), spec.min_height(theme));
                            ui.allocate_ui_with_layout(
                                size,
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| device::wells(ui, theme, spec, |_ui, _i| {}),
                            );
                        });
                        kit::gap(ui, theme, space::MD);
                    }
                });
                kit::gap(ui, theme, space::MD);
            }
        });
    }

    fn devices(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let cutoff = device::Param::hz("cutoff", 20.0, 20_000.0).with_default(1_000.0);
        let pan = device::Param::percent("pan").bipolar().with_default(50.0);
        let gain = device::Param::db("gain", -60.0, 6.0).with_default(0.0);
        let mix = device::Param::percent("mix").with_default(100.0);
        let res = device::Param::percent("res");
        // Discrete params: the step count comes from the name list, so the
        // two can never disagree about how many settings there are.
        let mode = device::Param::choice("mode", &["lp", "bp", "hp", "notch"]);
        let sync = device::Param::choice("sync", &["free", "sync"]);

        kit::section(ui, theme, "controls and readouts", |ui| {
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
            kit::muted(
                ui,
                theme,
                "value fields (drag to sweep, CLICK to type: \"1.5k\", \"250ms\", \"-6 dB\")",
            );
            ui.horizontal(|ui| {
                let delay = device::Param::ms("delay", 1.0, 4_000.0).with_default(250.0);
                for (param, value) in [
                    (&cutoff, &mut self.dev_field_hz),
                    (&delay, &mut self.dev_field_ms),
                    (&gain, &mut self.dev_field_db),
                ] {
                    kit::muted(ui, theme, param.name);
                    if device::field::field(ui, theme, param, value) {
                        self.log
                            .push(format!("{} -> {}", param.name, param.format(*value)));
                    }
                    kit::gap(ui, theme, space::MD);
                }
            });

            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "meter (click to clear the clip latch)");
            ui.horizontal(|ui| {
                self.dev_meter_t += ui.input(|i| i.stable_dt);
                let (l, r) = demo_levels(self.dev_meter_t);
                if device::meter::meter(ui, theme, &[l, r]) {
                    self.log.push("clip cleared".to_owned());
                }
                kit::gap(ui, theme, space::MD);
                // No side-by-side with `kit::meter`: it is a different
                // LENGTH (a fader's, not a meter's), so putting the two
                // together invites reading the difference as scale when
                // half of it is geometry. The numbers make the point —
                // -6 dB is near the top of a dB scale and halfway up a
                // linear one.
                kit::muted(
                    ui,
                    theme,
                    &format!("L {:>6} dBFS    R {:>6} dBFS", fmt_db(l), fmt_db(r)),
                );
                ui.ctx().request_repaint();
            });
            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "spectrum (tracks the cutoff knob)");
            let bins = demo_spectrum(cutoff.value(self.dev_cutoff));
            device::spectrum::spectrum(ui, theme, &bins, DEMO_NYQUIST_HZ);

            kit::gap(ui, theme, space::SM);
            kit::muted(
                ui,
                theme,
                "segmented switch (click, drag across, wheel, arrows once clicked)",
            );
            ui.horizontal(|ui| {
                if device::switch::switch(ui, theme, &mode, &mut self.dev_mode) {
                    self.log
                        .push(format!("mode -> {}", mode.format(self.dev_mode)));
                }
                kit::gap(ui, theme, space::MD);
                if device::switch::switch(ui, theme, &sync, &mut self.dev_sync) {
                    self.log
                        .push(format!("sync -> {}", sync.format(self.dev_sync)));
                }
            });

            kit::gap(ui, theme, space::SM);
            kit::row(ui, theme, "slider + readouts", |ui| {
                device::readout::readout_drag(ui, theme, &mix, &mut self.dev_mix);
                device::fader::slider(ui, theme, &mix, &mut self.dev_mix);
                device::readout::readout(ui, theme, &gain, self.dev_gain);
            });
        });

        kit::section(ui, theme, "curves and responses", |ui| {
            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "adsr (drag the handles)");
            device::envelope::adsr(ui, theme, &mut self.dev_env);

            kit::gap(ui, theme, space::SM);
            kit::muted(
                ui,
                theme,
                "filter response (drag the node: across = cutoff, up = resonance)",
            );
            let mode = device::Param::choice("mode", device::filter::Mode::NAMES);
            let slope = device::Param::choice("dB/oct", device::filter::Slope::NAMES);
            let drive = device::Param::percent("drive");
            ui.horizontal(|ui| {
                device::switch::switch(ui, theme, &mode, &mut self.dev_filter_mode);
                kit::gap(ui, theme, space::MD);
                // The slope switch is greyed by its own contract when the
                // mode does not use one — a notch has no 48 dB/octave.
                let uses = self.dev_filter.mode.uses_slope();
                ui.add_enabled_ui(uses, |ui| {
                    device::switch::switch(ui, theme, &slope, &mut self.dev_filter_slope);
                });
                kit::gap(ui, theme, space::MD);
                device::knob::knob(ui, theme, &drive, &mut self.dev_filter_drive);
            });
            self.dev_filter.mode =
                device::filter::Mode::from_index(mode.index(self.dev_filter_mode));
            self.dev_filter.slope =
                device::filter::Slope::from_index(slope.index(self.dev_filter_slope));
            self.dev_filter.drive = self.dev_filter_drive;
            if device::filter::filter_curve(ui, theme, &mut self.dev_filter, DEMO_NYQUIST_HZ * 2.0)
            {
                self.log.push(format!(
                    "filter -> {:.0} Hz  Q {:.2}",
                    self.dev_filter.cutoff_hz, self.dev_filter.q
                ));
            }
            kit::gap(ui, theme, space::SM);
            kit::muted(
                ui,
                theme,
                "waveshaper: every mode at once, then one you can drag",
            );
            {
                use device::shaper;
                // Every shape side by side. The comparison IS the point —
                // these are five different sounds, and seeing them
                // together is the fastest way to know which one you want.
                ui.horizontal(|ui| {
                    for (i, mode) in shaper::Mode::ALL.iter().enumerate() {
                        ui.vertical(|ui| {
                            kit::muted(ui, theme, shaper::Mode::NAMES[i]);
                            shaper::mini(
                                ui,
                                theme,
                                &shaper::Shaper {
                                    mode: *mode,
                                    drive: 6.0,
                                    bias: 0.0,
                                    mix: 1.0,
                                },
                            );
                        });
                        kit::gap(ui, theme, space::SM);
                    }
                });

                kit::gap(ui, theme, space::SM);
                let mode = device::Param::choice("mode", shaper::Mode::NAMES);
                let drive = device::Param::new(
                    "drive",
                    device::Mapping::Log {
                        min: shaper::DRIVE_MIN,
                        max: shaper::DRIVE_MAX,
                    },
                    device::Unit::Plain,
                );
                let bias = device::Param::percent("bias").bipolar().with_default(50.0);
                let mix = device::Param::percent("mix").with_default(100.0);
                ui.horizontal(|ui| {
                    device::switch::switch(ui, theme, &mode, &mut self.dev_shaper_mode);
                    kit::gap(ui, theme, space::MD);
                    device::knob::mini(ui, theme, &drive, &mut self.dev_shaper_drive);
                    kit::gap(ui, theme, space::SM);
                    device::knob::mini(ui, theme, &bias, &mut self.dev_shaper_bias);
                    kit::gap(ui, theme, space::SM);
                    device::knob::mini(ui, theme, &mix, &mut self.dev_shaper_mix);
                });
                self.dev_shaper.mode = shaper::Mode::from_index(mode.index(self.dev_shaper_mode));
                self.dev_shaper.drive = drive.value(self.dev_shaper_drive);
                // The bias knob is bipolar: 50% is centred, so it maps to
                // -BIAS_MAX..BIAS_MAX rather than 0..1.
                self.dev_shaper.bias = (self.dev_shaper_bias * 2.0 - 1.0) * shaper::BIAS_MAX;
                self.dev_shaper.mix = self.dev_shaper_mix;
                if shaper::transfer_curve(ui, theme, &mut self.dev_shaper) {
                    self.log.push(format!(
                        "shaper -> x{:.1} bias {:+.2}",
                        self.dev_shaper.drive, self.dev_shaper.bias
                    ));
                }
            }

            kit::gap(ui, theme, space::SM);
            kit::muted(
                ui,
                theme,
                "dynamics: one curve for compressor, limiter, gate and expander",
            );
            {
                use device::dynamics;
                let mode = device::Param::choice("mode", dynamics::Mode::NAMES);
                let thresh = device::Param::db("thresh", dynamics::VIEW_MIN_DB, 0.0);
                let ratio = device::Param::new(
                    "ratio",
                    device::Mapping::Log {
                        min: dynamics::RATIO_MIN,
                        max: dynamics::RATIO_MAX,
                    },
                    device::Unit::Plain,
                );
                let knee = device::Param::db("knee", 0.0, 24.0);

                ui.horizontal(|ui| {
                    device::switch::switch(ui, theme, &mode, &mut self.dev_dyn_mode);
                    kit::gap(ui, theme, space::MD);
                    device::knob::mini(ui, theme, &thresh, &mut self.dev_dyn_threshold);
                    kit::gap(ui, theme, space::SM);
                    device::knob::mini(ui, theme, &ratio, &mut self.dev_dyn_ratio);
                    kit::gap(ui, theme, space::SM);
                    device::knob::mini(ui, theme, &knee, &mut self.dev_dyn_knee);
                });
                self.dev_dyn.mode = dynamics::Mode::from_index(mode.index(self.dev_dyn_mode));
                self.dev_dyn.threshold_db = thresh.value(self.dev_dyn_threshold);
                self.dev_dyn.ratio = ratio.value(self.dev_dyn_ratio);
                self.dev_dyn.knee_db = knee.value(self.dev_dyn_knee);

                // The live operating point rides the meter demo's signal,
                // so the dot and the reduction reading actually move.
                let (l, _) = demo_levels(self.dev_meter_t);
                let level = device::meter::amp_to_db(l).max(dynamics::VIEW_MIN_DB);
                // The BAR view, with attack and release beside it — the
                // two settings a static transfer curve cannot show, next
                // to the display that can.
                let attack = device::Param::ms("attack", 0.1, 300.0);
                let release = device::Param::ms("release", 5.0, 2_000.0);
                ui.horizontal(|ui| {
                    if device::knob::mini(ui, theme, &attack, &mut self.dev_dyn_attack) {
                        self.dev_dyn.attack_ms = attack.value(self.dev_dyn_attack);
                    }
                    kit::gap(ui, theme, space::SM);
                    if device::knob::mini(ui, theme, &release, &mut self.dev_dyn_release) {
                        self.dev_dyn.release_ms = release.value(self.dev_dyn_release);
                    }
                    kit::gap(ui, theme, space::MD);
                    dynamics::bars(ui, theme, &mut self.dev_dyn, level);
                });
                self.dev_dyn.attack_ms = attack.value(self.dev_dyn_attack);
                self.dev_dyn.release_ms = release.value(self.dev_dyn_release);

                ui.horizontal(|ui| {
                    if dynamics::transfer_curve(ui, theme, &mut self.dev_dyn, Some(level)) {
                        self.log.push(format!(
                            "dyn -> {:.0} dB  {:.1}:1",
                            self.dev_dyn.threshold_db, self.dev_dyn.ratio
                        ));
                    }
                    kit::gap(ui, theme, space::SM);
                    dynamics::reduction_meter(ui, theme, self.dev_dyn.reduction_db(level));
                    kit::gap(ui, theme, space::MD);
                    dynamics::mini(ui, theme, &self.dev_dyn);
                });
                ui.ctx().request_repaint();
            }
        });

        kit::section(ui, theme, "device cards", |ui| {
            kit::gap(ui, theme, space::SM);
            kit::muted(
                ui,
                theme,
                "poly synth studies — the picture is the control (drag, wheel, arrows)",
            );
            ui.horizontal(|ui| {
                if device::poly_widgets::wave_picker(ui, theme, &mut self.dev_poly.osc_a.wave) {
                    self.log.push("PolyWave changed".to_owned());
                }
                kit::gap(ui, theme, space::MD);
                if device::poly_widgets::pitch_stack(
                    ui,
                    theme,
                    &mut self.dev_poly.osc_a.octave,
                    &mut self.dev_poly.osc_a.semi,
                    &mut self.dev_poly.osc_a.fine,
                ) {
                    self.log.push("PolyPitch changed".to_owned());
                }
                kit::gap(ui, theme, space::MD);
                let unison_norm = self.dev_poly.unison.clamp(0.0, 1.0);
                let last = device::poly_widgets::UNISON_MAX - 1;
                let voices =
                    params::poly::unison((unison_norm * last as f32).round() as u32) as usize;
                if device::poly_widgets::unison_field(
                    ui,
                    theme,
                    voices,
                    &mut self.dev_poly.spread,
                    &mut self.dev_poly.detune,
                ) {
                    self.log.push("PolyUnison changed".to_owned());
                }
            });

            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "the mini curve, in a card beside its knobs");
            device::card(ui, theme, "filter", |ui| {
                // The first card built entirely from the new widgets, and
                // the reason the mini is a fixed size: it declares a
                // footprint like anything else, so the well holding it is
                // sized by contract rather than by hope.
                let res = device::Param::percent("res");
                let drive = device::Param::percent("drive");
                let mix = device::Param::percent("mix");
                // A 2x2 of MINI knobs beside the thumbnail — a row split
                // a card could not hold until the minis existed, and the
                // reason they do.
                let mini_fp = device::knob::footprint_mini(ui, theme, &cutoff)
                    .union(device::knob::footprint_mini(ui, theme, &res))
                    .union(device::knob::footprint_mini(ui, theme, &drive))
                    .union(device::knob::footprint_mini(ui, theme, &mix));
                // COMPACT, because everything in it is a mini: half-step
                // padding, tighter gaps, no hairlines. The contract is
                // built at the density it will be drawn at — `each` here
                // instead of `each_compact` would reserve for chrome that
                // never gets drawn.
                let layout = device::Wells::new()
                    .row([
                        device::Well::span(2)
                            .fits(device::filter::footprint_mini(theme))
                            .titled("response", ui, theme),
                        device::Well::divided(2, 2)
                            .each_compact(mini_fp, theme)
                            .titled("shape", ui, theme),
                    ])
                    .compact();
                device::wells(ui, theme, &layout, |ui, i| match i {
                    0 => device::filter::mini(ui, theme, &self.dev_filter, DEMO_NYQUIST_HZ * 2.0),
                    1 => {
                        if device::knob::mini(ui, theme, &cutoff, &mut self.dev_cutoff) {
                            self.dev_filter.cutoff_hz = cutoff.value(self.dev_cutoff);
                        }
                    }
                    2 => {
                        let mut q = ((self.dev_filter.q - 0.3) / 23.7).clamp(0.0, 1.0);
                        if device::knob::mini(ui, theme, &res, &mut q) {
                            self.dev_filter.q = 0.3 + q * 23.7;
                        }
                    }
                    3 => {
                        if device::knob::mini(ui, theme, &drive, &mut self.dev_filter_drive) {
                            self.dev_filter.drive = self.dev_filter_drive;
                        }
                    }
                    _ => {
                        device::knob::mini(ui, theme, &mix, &mut self.dev_mix);
                    }
                });
            });

            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "sine synth — the first real device");
            for edit in device::sine_synth_card(ui, theme, &mut self.dev_synth) {
                self.log.push(format!(
                    "SynthEdit(param {}, {:.2})",
                    edit.param, edit.value
                ));
            }

            kit::gap(ui, theme, space::SM);
            kit::muted(
                ui,
                theme,
                "poly synth — one tab page per section of the voice path",
            );
            for edit in device::poly_card(ui, theme, &mut self.dev_poly) {
                self.log
                    .push(format!("PolyEdit(param {}, {:.2})", edit.param, edit.value));
            }

            kit::gap(ui, theme, space::SM);
            kit::muted(
                ui,
                theme,
                "lo-fi — the staircase is the real kernel, run on a real sine",
            );
            for edit in device::lofi_card(ui, theme, &mut self.dev_lofi) {
                self.log
                    .push(format!("LofiEdit(param {}, {:.2})", edit.param, edit.value));
            }

            kit::gap(ui, theme, space::SM);
            kit::muted(
                ui,
                theme,
                "sheen — what the brightener adds, on two synthetic hits",
            );
            for edit in device::sheen_card(ui, theme, &mut self.dev_sheen) {
                self.log.push(format!(
                    "SheenEdit(param {}, {:.2})",
                    edit.param, edit.value
                ));
            }

            kit::gap(ui, theme, space::SM);
            kit::muted(
                ui,
                theme,
                "disperser — a click, and what the allpass chain makes of it",
            );
            for edit in device::disperser_card(ui, theme, &mut self.dev_disperser) {
                self.log.push(format!(
                    "DisperserEdit(param {}, {:.2})",
                    edit.param, edit.value
                ));
            }

            kit::gap(ui, theme, space::SM);
            kit::muted(ui, theme, "tilt — the see-saw, measured through a real FFT");
            for edit in device::tilt_card(ui, theme, &mut self.dev_tilt) {
                self.log
                    .push(format!("TiltEdit(param {}, {:.2})", edit.param, edit.value));
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
/// A synthetic stereo level: a slow swell that occasionally overshoots
/// full scale, so the clip latch has something to catch.
fn demo_levels(t: f32) -> (f32, f32) {
    let swell = |phase: f32| {
        let slow = (t * 0.9 + phase).sin() * 0.5 + 0.5;
        // Cubed, so the quiet parts are genuinely quiet — a level that
        // never leaves the top of the scale demonstrates nothing.
        let base = slow * slow * slow;
        // An occasional overshoot past 1.0.
        let spike = if ((t * 0.37 + phase).sin()) > 0.985 {
            0.4
        } else {
            0.0
        };
        (base * 1.05 + spike).clamp(0.0, 1.6)
    };
    (swell(0.0), swell(1.1))
}

/// The level as text, matching what the meter is showing.
fn fmt_db(amp: f32) -> String {
    let db = device::meter::amp_to_db(amp);
    if db <= device::meter::FLOOR_DB {
        "-inf".to_owned()
    } else {
        format!("{db:+.1}")
    }
}

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
