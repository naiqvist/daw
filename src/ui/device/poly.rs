//! The poly synth device card — the UI face of the workhorse synth
//! described in `notes/20260825-synth-brief.md`.
//!
//! # One instrument, four pages
//!
//! The card is one fixed piece of equipment. Its named tab rail follows the
//! signal path — oscillator, filter, amp, modulation — while the silhouette,
//! title and touch cartridge never move. Osc A/B remain a local choice inside
//! the oscillator screen.
//!
//! # One table, dense ids, no match arms
//!
//! Ids, names, ranges and defaults all come from [`crate::params::poly`]
//! — the one table this widget, `Node::Poly::apply` and the app's edit
//! routing will all read. Because that table's ids are dense and equal to
//! their index (an invariant `params`' own tests enforce), the widget can
//! keep its descriptions in a flat `[Param; N]` and its knob positions
//! reachable by id, so a well is declared as a LIST OF IDS rather than as
//! a match with one arm per control. A control that exists in the layout
//! but not in the list is then a length mismatch a test can see, instead
//! of an arm someone forgot to write.

use crate::params::poly::{
    A_FINE, A_LEVEL, A_OCT, A_PENV, A_SEMI, A_WAVE, AMP_A, AMP_D, AMP_R, AMP_S, B_FINE, B_LEVEL,
    B_OCT, B_PENV, B_SEMI, B_WAVE, DRIVE_POS, F_CUTOFF, F_DRIVE, F_ENV, F_KEY, F_MODE, F_POS,
    F_RES, F_SLOPE, FENV_A, FENV_D, FENV_R, FENV_S, GAIN, MOD_DST, MOD_SRC, N_COLOR, N_DECAY,
    N_LEVEL, NOISE_COLORS, OCTAVES, PENV_D, TABLE, UNISON, V_DETUNE, V_GLIDE, V_MODE, V_SPREAD,
    V_UNISON, VEL, VOICE_MODES, W1_AMT, W1_DST, W1_SRC, W2_AMT, W2_DST, W2_SRC, W3_AMT, W3_DST,
    W3_SRC, WAVES,
};
use crate::params::{self, poly::label};
use crate::ui::device::envelope::Adsr;
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Param, Well, Wells, card, envelope, filter, param, poly_widgets, switch};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, space};
use eframe::egui;

/// How many parameters the synth has. The table's length, so adding a row
/// grows the widget's storage with it.
const N: usize = TABLE.len();

/// Sample rate the filter curve is drawn against. The display is a
/// picture of the response, not the engine's own filter state, so a fixed
/// rate keeps the drawing stable until the node exists to supply the
/// stream's real rate.
const CURVE_SAMPLE_RATE: f32 = 48_000.0;

// --------------------------------------------------------------- state ---

/// Knob positions of one oscillator, normalized.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OscUi {
    pub wave: f32,
    pub octave: f32,
    pub semi: f32,
    pub fine: f32,
    pub level: f32,
    /// How much the pitch envelope moves this oscillator. The kick
    /// recipe lives here.
    pub pitch_env: f32,
}

impl Default for OscUi {
    fn default() -> Self {
        // Osc A's defaults. Osc B differs only in level, and says so at
        // the one place it differs rather than by having its own table.
        Self {
            wave: default_norm(A_WAVE),
            octave: default_norm(A_OCT),
            semi: default_norm(A_SEMI),
            fine: default_norm(A_FINE),
            level: default_norm(A_LEVEL),
            pitch_env: default_norm(A_PENV),
        }
    }
}

/// Knob positions of one poly synth. Serialized into project files with
/// NAMED fields, so a patch survives both a reload and a future
/// renumbering of the table — the wire ids are dense and positional, and
/// a project file must not be.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PolyUi {
    /// Persisted navigation: bit 0 selects oscillator B and bits 1–2 select
    /// the main instrument page. Keeping oscillator selection in its original
    /// bit makes project files written by the former single-face UI migrate
    /// to the OSC page without losing which oscillator was open.
    pub page: usize,

    pub osc_a: OscUi,
    pub osc_b: OscUi,

    pub noise_color: f32,
    pub noise_level: f32,
    pub noise_decay: f32,

    pub filter_mode: f32,
    pub filter_slope: f32,
    pub cutoff: f32,
    pub res: f32,
    pub filter_env: f32,
    pub keytrack: f32,
    pub drive: f32,
    pub drive_pos: f32,

    pub amp_attack: f32,
    pub amp_decay: f32,
    pub amp_sustain: f32,
    pub amp_release: f32,
    pub gain: f32,
    pub velocity: f32,

    pub voice_mode: f32,
    pub glide: f32,
    pub unison: f32,
    pub detune: f32,
    pub spread: f32,

    pub fenv_attack: f32,
    pub fenv_decay: f32,
    pub fenv_sustain: f32,
    pub fenv_release: f32,
    pub penv_decay: f32,

    /// The matrix rows, `[src, dst, depth]` per wire, normalized like
    /// every other knob here.
    pub wires: [[f32; 3]; 3],
}

impl Default for PolyUi {
    fn default() -> Self {
        // Osc B opens SILENT. Two oscillators both at full level is a
        // patch nobody asked for — the second one is an addition the user
        // makes, not a default they have to undo.
        let osc_b = OscUi {
            level: default_norm(B_LEVEL),
            ..OscUi::default()
        };
        Self {
            page: 0,
            osc_a: OscUi::default(),
            osc_b,
            noise_color: default_norm(N_COLOR),
            noise_level: default_norm(N_LEVEL),
            noise_decay: default_norm(N_DECAY),
            filter_mode: default_norm(F_MODE),
            filter_slope: default_norm(F_SLOPE),
            cutoff: default_norm(F_CUTOFF),
            res: default_norm(F_RES),
            filter_env: default_norm(F_ENV),
            keytrack: default_norm(F_KEY),
            drive: default_norm(F_DRIVE),
            drive_pos: default_norm(F_POS),
            amp_attack: default_norm(AMP_A),
            amp_decay: default_norm(AMP_D),
            amp_sustain: default_norm(AMP_S),
            amp_release: default_norm(AMP_R),
            gain: default_norm(GAIN),
            velocity: default_norm(VEL),
            voice_mode: default_norm(V_MODE),
            glide: default_norm(V_GLIDE),
            unison: default_norm(V_UNISON),
            detune: default_norm(V_DETUNE),
            spread: default_norm(V_SPREAD),
            fenv_attack: default_norm(FENV_A),
            fenv_decay: default_norm(FENV_D),
            fenv_sustain: default_norm(FENV_S),
            fenv_release: default_norm(FENV_R),
            penv_decay: default_norm(PENV_D),
            wires: [[
                default_norm(W1_SRC),
                default_norm(W1_DST),
                default_norm(W1_AMT),
            ]; 3],
        }
    }
}

impl PolyUi {
    /// The knob position of one parameter, by wire id.
    ///
    /// THE map between the table and this struct, and the only one: a
    /// reader is the writer applied to a copy, which is honest because
    /// `PolyUi` is `Copy` and cheap because it is 25 floats. Writing the
    /// match twice is how the two halves drift.
    pub fn slot(&mut self, id: u32) -> Option<&mut f32> {
        let slot = match id {
            A_WAVE => &mut self.osc_a.wave,
            A_OCT => &mut self.osc_a.octave,
            A_SEMI => &mut self.osc_a.semi,
            A_FINE => &mut self.osc_a.fine,
            A_LEVEL => &mut self.osc_a.level,
            A_PENV => &mut self.osc_a.pitch_env,
            B_WAVE => &mut self.osc_b.wave,
            B_OCT => &mut self.osc_b.octave,
            B_SEMI => &mut self.osc_b.semi,
            B_FINE => &mut self.osc_b.fine,
            B_LEVEL => &mut self.osc_b.level,
            B_PENV => &mut self.osc_b.pitch_env,
            N_COLOR => &mut self.noise_color,
            N_LEVEL => &mut self.noise_level,
            N_DECAY => &mut self.noise_decay,
            F_MODE => &mut self.filter_mode,
            F_SLOPE => &mut self.filter_slope,
            F_CUTOFF => &mut self.cutoff,
            F_RES => &mut self.res,
            F_ENV => &mut self.filter_env,
            F_KEY => &mut self.keytrack,
            F_DRIVE => &mut self.drive,
            F_POS => &mut self.drive_pos,
            AMP_A => &mut self.amp_attack,
            AMP_D => &mut self.amp_decay,
            AMP_S => &mut self.amp_sustain,
            AMP_R => &mut self.amp_release,
            GAIN => &mut self.gain,
            VEL => &mut self.velocity,
            V_MODE => &mut self.voice_mode,
            V_GLIDE => &mut self.glide,
            V_UNISON => &mut self.unison,
            V_DETUNE => &mut self.detune,
            V_SPREAD => &mut self.spread,
            FENV_A => &mut self.fenv_attack,
            FENV_D => &mut self.fenv_decay,
            FENV_S => &mut self.fenv_sustain,
            FENV_R => &mut self.fenv_release,
            PENV_D => &mut self.penv_decay,
            W1_SRC => &mut self.wires[0][0],
            W1_DST => &mut self.wires[0][1],
            W1_AMT => &mut self.wires[0][2],
            W2_SRC => &mut self.wires[1][0],
            W2_DST => &mut self.wires[1][1],
            W2_AMT => &mut self.wires[1][2],
            W3_SRC => &mut self.wires[2][0],
            W3_DST => &mut self.wires[2][1],
            W3_AMT => &mut self.wires[2][2],
            _ => return None,
        };
        Some(slot)
    }

    /// [`Self::slot`] for a reader.
    pub fn norm(&self, id: u32) -> Option<f32> {
        let mut copy = *self;
        copy.slot(id).map(|v| *v)
    }

    /// Which oscillator the OSC page edits — page bit 0.
    fn oscillator(&self) -> usize {
        self.page & 1
    }

    fn show_oscillator(&mut self, oscillator: usize) {
        self.page = (self.page & !1) | oscillator.min(1);
    }

    /// The named top-level instrument page — bits 1 and 2.
    fn main_page(&self) -> usize {
        (self.page >> 1) & 0b11
    }

    fn show_main_page(&mut self, page: usize) {
        self.page = (self.page & 1) | (page.min(3) << 1);
    }

    /// The amp envelope as the editor wants it. The editor's handles are
    /// normalized stage positions, which is exactly what the table's
    /// times and level map from — so the envelope IS the four knobs, not
    /// a second representation of them.
    fn amp(&self) -> Adsr {
        Adsr {
            attack: self.amp_attack,
            decay: self.amp_decay,
            sustain: self.amp_sustain,
            release: self.amp_release,
        }
    }

    /// The filter envelope as the editor wants it — same shape as the
    /// amp's, different four knobs.
    fn fenv(&self) -> Adsr {
        Adsr {
            attack: self.fenv_attack,
            decay: self.fenv_decay,
            sustain: self.fenv_sustain,
            release: self.fenv_release,
        }
    }

    /// The filter, in the natural units the response curve draws.
    fn filter(&self, s: &Spec) -> filter::Filter {
        filter::Filter {
            mode: filter::Mode::from_index(s.get(F_MODE).index(self.filter_mode)),
            slope: filter::Slope::from_index(s.get(F_SLOPE).index(self.filter_slope)),
            cutoff_hz: s.get(F_CUTOFF).value(self.cutoff),
            q: s.get(F_RES).value(self.res),
            drive: s.get(F_DRIVE).value(self.drive) / 100.0,
            // The synth's filter has no character switch: its voicing is
            // the one the curve has always drawn, and CLEAN is that
            // voicing written down rather than a choice made here.
            character: crate::params::filter::CHAR_CLEAN,
        }
    }
}

// ---------------------------------------------------------------- spec ---

/// How a table row becomes a widget mapping. The row already carries the
/// range and the default; this is the ONE thing it does not say — what
/// the number means to a reader.
#[derive(Clone, Copy)]
enum Kind {
    Choice(&'static [&'static str]),
    Hz,
    Ms,
    Percent,
    Semitones,
    /// Ratio-like and positive: equal knob travel is equal ratio.
    Log,
    /// A bare number. Cents and linear gain, until `Unit` has arms for
    /// them.
    Plain,
}

/// The unit of every parameter, by wire id. `None` for an id the table
/// does not have — and, deliberately, for an id it DOES have that nobody
/// gave a unit to, which is what the completeness test catches.
fn kind(id: u32) -> Option<Kind> {
    Some(match id {
        A_WAVE | B_WAVE => Kind::Choice(WAVES),
        A_OCT | B_OCT => Kind::Choice(OCTAVES),
        N_COLOR => Kind::Choice(NOISE_COLORS),
        F_MODE => Kind::Choice(filter::Mode::NAMES),
        F_SLOPE => Kind::Choice(filter::Slope::NAMES),
        F_POS => Kind::Choice(DRIVE_POS),
        V_MODE => Kind::Choice(VOICE_MODES),
        V_UNISON => Kind::Choice(UNISON),
        W1_SRC | W2_SRC | W3_SRC => Kind::Choice(MOD_SRC),
        W1_DST | W2_DST | W3_DST => Kind::Choice(MOD_DST),
        A_SEMI | B_SEMI | A_PENV | B_PENV => Kind::Semitones,
        A_FINE | B_FINE => Kind::Plain,
        A_LEVEL | B_LEVEL | N_LEVEL | F_ENV | F_KEY | F_DRIVE | AMP_S | FENV_S | VEL | V_DETUNE
        | V_SPREAD | W1_AMT | W2_AMT | W3_AMT => Kind::Percent,
        N_DECAY | AMP_A | AMP_D | AMP_R | FENV_A | FENV_D | FENV_R | PENV_D | V_GLIDE => Kind::Ms,
        F_CUTOFF => Kind::Hz,
        F_RES => Kind::Log,
        GAIN => Kind::Plain,
        _ => return None,
    })
}

/// Build one parameter's description from its table row.
///
/// The row's range IS the mapping and the row's default IS the default,
/// so a widget cannot show a value the engine would refuse. Polarity is
/// derived rather than declared: a range that reaches below zero is
/// bipolar, which is the definition, not a convention to keep in sync.
fn build(id: u32) -> Param {
    let d = params::def(TABLE, id);
    let name = label(d.name);
    let linear = param::Mapping::Linear {
        min: d.min,
        max: d.max,
    };
    let p = match kind(id) {
        Some(Kind::Choice(names)) => Param::choice(name, names),
        Some(Kind::Hz) => Param::hz(name, d.min, d.max),
        Some(Kind::Ms) => Param::ms(name, d.min, d.max),
        Some(Kind::Percent) => Param::new(name, linear, param::Unit::Percent),
        Some(Kind::Semitones) => Param::new(name, linear, param::Unit::Semitones),
        Some(Kind::Log) => Param::new(
            name,
            param::Mapping::Log {
                min: d.min,
                max: d.max,
            },
            param::Unit::Plain,
        ),
        Some(Kind::Plain) | None => Param::new(name, linear, param::Unit::Plain),
    }
    .with_default(d.default);
    if d.min < 0.0 { p.bipolar() } else { p }
}

/// The knob position a parameter's table default sits at.
fn default_norm(id: u32) -> f32 {
    build(id).default_norm
}

/// Every parameter description the card draws, indexed BY WIRE ID.
///
/// Dense ids make this an array rather than a match, which is the whole
/// reason a well can be declared as a list of ids.
struct Spec {
    params: [Param; N],
}

impl Spec {
    fn get(&self, id: u32) -> &Param {
        // Clamped rather than panicking: the ids all come from consts, so
        // an out-of-range one is a programming error, and drawing the
        // wrong knob is a better way to find it than killing the frame.
        &self.params[(id as usize).min(N - 1)]
    }
}

/// Built fresh per frame, like the other device cards — 34 small `Copy`
/// descriptions read straight out of a static table.
fn spec() -> Spec {
    let mut params = [build(0); N];
    for (i, slot) in params.iter_mut().enumerate() {
        *slot = build(i as u32);
    }
    Spec { params }
}

// ---------------------------------------------------------------- draw ---

fn push_edit(s: &Spec, state: &PolyUi, id: u32, out: &mut Vec<ParamEdit>) {
    if let Some(norm) = state.norm(id) {
        out.push(ParamEdit {
            param: id,
            value: s.get(id).value(norm),
        });
    }
}
fn edit_adsr(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Spec,
    state: &mut PolyUi,
    amp: bool,
    fill: bool,
    out: &mut Vec<ParamEdit>,
) {
    let (mut env, rows) = if amp {
        (
            state.amp(),
            [(AMP_A, 0usize), (AMP_D, 1), (AMP_S, 2), (AMP_R, 3)],
        )
    } else {
        (
            state.fenv(),
            [(FENV_A, 0usize), (FENV_D, 1), (FENV_S, 2), (FENV_R, 3)],
        )
    };
    let changed = if fill {
        envelope::adsr_fill(ui, theme, &mut env)
    } else {
        envelope::adsr_compact(ui, theme, &mut env)
    };
    if changed {
        let stages = [env.attack, env.decay, env.sustain, env.release];
        for (id, at) in rows {
            if let Some(slot) = state.slot(id) {
                *slot = stages[at];
            }
            push_edit(s, state, id, out);
        }
    }
}

/// A percent level as the decibels it is heard in. Display only — the
/// wire stays the table's 0–100.
fn level_db(pct: f32) -> String {
    if pct <= 0.0 {
        "−∞ dB".to_owned()
    } else {
        format!("{:+.1} dB", 20.0 * (pct / 100.0).log10())
    }
}

/// A linear gain as decibels — unity is +0.0.
fn gain_db(gain: f32) -> String {
    if gain <= 0.0 {
        "−∞ dB".to_owned()
    } else {
        format!("{:+.1} dB", 20.0 * gain.log10())
    }
}

/// One row of Wavetable-style cells — value over whisper label — each
/// `size.x` wide, `size.y` tall. Levels get the dB face.
fn labeled_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Spec,
    state: &mut PolyUi,
    ids: &[u32],
    size: egui::Vec2,
    out: &mut Vec<ParamEdit>,
) {
    let (width, height) = (size.x, size.y);
    let gap = theme.sp(space::XXS);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for id in ids {
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    ui.set_height(height);
                    let param = *s.get(*id);
                    let db = matches!(*id, A_LEVEL | B_LEVEL | N_LEVEL);
                    let fmt: Option<&dyn Fn(f32) -> String> =
                        if db { Some(&level_db) } else { None };
                    if let Some(norm) = state.slot(*id)
                        && poly_widgets::labeled_cell(ui, theme, &param, norm, fmt)
                    {
                        push_edit(s, state, *id, out);
                    }
                },
            );
        }
    });
}

/// One corner slide bound to a table row: format via the param (or a
/// custom face like dB), edit the stored norm, emit the letter.
#[allow(clippy::too_many_arguments)]
fn overlay_slide(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Spec,
    state: &mut PolyUi,
    id: u32,
    anchor: egui::Pos2,
    align: egui::Align2,
    label: &str,
    fmt: Option<&dyn Fn(f32) -> String>,
    hint: &str,
    out: &mut Vec<ParamEdit>,
) {
    let param = *s.get(id);
    let Some(norm) = state.slot(id) else { return };
    let value = match fmt {
        Some(f) => f(param.value(*norm)),
        None => param.format(*norm),
    };
    if poly_widgets::corner_slide(
        ui,
        theme,
        anchor,
        align,
        &format!("overlay-{id}"),
        format!("{label} {value}"),
        norm,
        param.default_norm,
        hint,
    ) {
        push_edit(s, state, id, out);
    }
}

/// One corner menu bound to a table row.
#[allow(clippy::too_many_arguments)]
fn overlay_menu(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Spec,
    state: &mut PolyUi,
    id: u32,
    anchor: egui::Pos2,
    align: egui::Align2,
    out: &mut Vec<ParamEdit>,
) {
    let param = *s.get(id);
    let Some(norm) = state.slot(id) else { return };
    if poly_widgets::corner_menu(
        ui,
        theme,
        anchor,
        align,
        &format!("overlay-menu-{id}"),
        &param,
        norm,
    ) {
        push_edit(s, state, id, out);
    }
}

fn oscillator_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Spec,
    state: &mut PolyUi,
    out: &mut Vec<ParamEdit>,
) {
    ui.spacing_mut().item_spacing.y = theme.sp(space::XXS);
    let wave_of = |id: u32| s.get(id).index(state.norm(id).unwrap_or_default());
    let (a_wave, b_wave) = (wave_of(A_WAVE), wave_of(B_WAVE));
    let selected = state.oscillator();
    // Three honest cells and nothing else: the pitch envelope became the
    // drop glyph, and the level rides the hero's corner in dB.
    let (wave_id, level_id, penv_id, ids) = if selected == 0 {
        (A_WAVE, A_LEVEL, A_PENV, [A_OCT, A_SEMI, A_FINE])
    } else {
        (B_WAVE, B_LEVEL, B_PENV, [B_OCT, B_SEMI, B_FINE])
    };
    let mut switch_to = None;
    let mut wave_step = 0i32;
    let mut wave_pick = None;

    // The WELL holds only the oscillator: the hero on top, the selected
    // oscillator's own values beneath it, each value over its whisper
    // label — Wavetable's grammar. Everything about voices and noise
    // lives OUTSIDE the well in open air below, under its own section
    // word: the dark surface says "this is the oscillator", the air says
    // "this is how the instrument plays it".
    let pair_h = theme.sp(control::POLY_CELL_H) * 2.0;
    let gap = theme.sp(space::XS);
    // The hero owns the panel: the well takes every point the single air
    // row below does not need, and the row packs the whole VOICE group
    // plus the noise THUMBNAIL — four parameters living as one drawn,
    // draggable burst instead of four labelled cells and a title.
    let air_h = pair_h + gap * 2.0;
    let panel_h = (ui.available_height() - air_h).max(theme.sp(control::POLY_WAVE_H) + pair_h);
    let panel_w = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(panel_w, panel_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_height(panel_h);
            ui.set_width(panel_w);
            poly_widgets::dark_curve_panel(
                ui,
                theme,
                None,
                control::POLY_WAVE_H,
                0,
                2,
                |ui, region| match region {
                    poly_widgets::CurveRegion::Plot => {
                        let plot = ui.max_rect().shrink(theme.sp(space::XS));
                        (switch_to, wave_step, wave_pick) =
                            poly_widgets::wave_hero(ui, theme, a_wave, b_wave, selected);
                        // The selected oscillator's LEVEL, in dB, on the
                        // display it levels — under its own corner tag.
                        overlay_slide(
                            ui,
                            theme,
                            s,
                            state,
                            level_id,
                            plot.left_bottom(),
                            egui::Align2::LEFT_BOTTOM,
                            "lvl",
                            Some(&level_db),
                            "oscillator level — drag",
                            out,
                        );
                        overlay_slide(
                            ui,
                            theme,
                            s,
                            state,
                            V_GLIDE,
                            plot.right_bottom(),
                            egui::Align2::RIGHT_BOTTOM,
                            "gld",
                            None,
                            "glide — drag; heard in mono and legato",
                            out,
                        );
                    }
                    poly_widgets::CurveRegion::Footer => {
                        let width = ((ui.available_width() - gap * (ids.len() - 1) as f32)
                            / ids.len() as f32)
                            .max(0.0);
                        labeled_row(ui, theme, s, state, &ids, egui::vec2(width, pair_h), out);
                    }
                    poly_widgets::CurveRegion::Header => {}
                },
            );
        },
    );

    ui.add_space(theme.sp(space::XXS));

    // One SHORT air row: three living thumbnails. The pitch drop holds
    // the selected oscillator's envelope depth and the shared decay; the
    // voice cloud holds mode, count, detune, spread and glide; the noise
    // burst holds color, level and decay.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let glyph_w = ((panel_w - gap * 2.0) / 3.0).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(glyph_w, pair_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(glyph_w);
                ui.set_height(pair_h);
                let (depth_p, decay_p) = (*s.get(penv_id), *s.get(PENV_D));
                let mut depth = state.norm(penv_id).unwrap_or_default();
                let mut decay = state.norm(PENV_D).unwrap_or_default();
                let changed = poly_widgets::pitch_drop_glyph(
                    ui, theme, &depth_p, &mut depth, &decay_p, &mut decay,
                );
                for (moved, id, norm) in [(changed[0], penv_id, depth), (changed[1], PENV_D, decay)]
                {
                    if moved {
                        if let Some(slot) = state.slot(id) {
                            *slot = norm;
                        }
                        push_edit(s, state, id, out);
                    }
                }
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(glyph_w, pair_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(glyph_w);
                ui.set_height(pair_h);
                let (mode_p, uni_p, det_p, spr_p) = (
                    *s.get(V_MODE),
                    *s.get(V_UNISON),
                    *s.get(V_DETUNE),
                    *s.get(V_SPREAD),
                );
                let mut mode = state.norm(V_MODE).unwrap_or_default();
                let mut unison = state.norm(V_UNISON).unwrap_or_default();
                let mut detune = state.norm(V_DETUNE).unwrap_or_default();
                let mut spread = state.norm(V_SPREAD).unwrap_or_default();
                let changed = poly_widgets::voice_glyph(
                    ui,
                    theme,
                    &mode_p,
                    &mut mode,
                    &uni_p,
                    &mut unison,
                    &det_p,
                    &mut detune,
                    &spr_p,
                    &mut spread,
                );
                for (moved, id, norm) in [
                    (changed[0], V_MODE, mode),
                    (changed[1], V_UNISON, unison),
                    (changed[2], V_DETUNE, detune),
                    (changed[3], V_SPREAD, spread),
                ] {
                    if moved {
                        if let Some(slot) = state.slot(id) {
                            *slot = norm;
                        }
                        push_edit(s, state, id, out);
                    }
                }
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(glyph_w, pair_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(glyph_w);
                ui.set_height(pair_h);
                let (color_p, level_p, decay_p) =
                    (*s.get(N_COLOR), *s.get(N_LEVEL), *s.get(N_DECAY));
                let mut color = state.norm(N_COLOR).unwrap_or_default();
                let mut level = state.norm(N_LEVEL).unwrap_or_default();
                let mut decay = state.norm(N_DECAY).unwrap_or_default();
                let changed = poly_widgets::noise_glyph(
                    ui, theme, &color_p, &mut color, &level_p, &mut level, &decay_p, &mut decay,
                );
                for (moved, id, norm) in [
                    (changed[0], N_COLOR, color),
                    (changed[1], N_LEVEL, level),
                    (changed[2], N_DECAY, decay),
                ] {
                    if moved {
                        if let Some(slot) = state.slot(id) {
                            *slot = norm;
                        }
                        push_edit(s, state, id, out);
                    }
                }
            },
        );
    });

    if let Some(which) = switch_to {
        state.show_oscillator(which);
    }
    if wave_step != 0 || wave_pick.is_some() {
        let param = s.get(wave_id);
        let at = match wave_pick {
            Some(pick) => pick.min(crate::params::poly::WAVES.len() - 1),
            None => (param.index(state.norm(wave_id).unwrap_or_default()) as i32 + wave_step)
                .clamp(0, crate::params::poly::WAVES.len() as i32 - 1) as usize,
        };
        let next = param.at_index(at);
        if state.norm(wave_id) != Some(next) {
            if let Some(slot) = state.slot(wave_id) {
                *slot = next;
            }
            push_edit(s, state, wave_id, out);
        }
    }
}

fn filter_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Spec,
    state: &mut PolyUi,
    out: &mut Vec<ParamEdit>,
) {
    let gap = theme.sp(space::XXS);
    ui.spacing_mut().item_spacing.y = gap;

    // Both visuals span the full filter well. The envelope needs neither a
    // title nor four written stage controls: its draggable curve explains
    // itself and receives every point reclaimed from that annotation row.
    let response_plot_h = control::CURVE_COMPACT_H - 4.0;
    let envelope_plot_h = control::ENV_COMPACT_H + control::POLY_CELL_H + space::XXS;
    // No row budget: both surfaces carry their settings as corner
    // annotations now, so every point goes to the plots themselves.
    let response_min = theme.sp(response_plot_h);
    let envelope_min = theme.sp(envelope_plot_h);
    let extra = (ui.available_height() - response_min - envelope_min - gap).max(0.0);
    let response_h = response_min + extra * 0.6;
    let envelope_h = envelope_min + extra * 0.4;
    let width = ui.available_width();

    ui.allocate_ui_with_layout(
        egui::vec2(width, response_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.set_height(response_h);
            // NO cell rows at all: the response display carries its own
            // settings as corner annotations. Mode and slope are chips on
            // the shape they describe; drive and its position share the
            // bottom-left; keytrack rides bottom-right. Cutoff and
            // resonance were already the node itself.
            poly_widgets::dark_curve_panel(ui, theme, None, response_plot_h, 0, 0, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        let plot = ui.max_rect();
                        let mut f = state.filter(s);
                        if filter::filter_curve_fill(ui, theme, &mut f, CURVE_SAMPLE_RATE) {
                            for (id, value) in [(F_CUTOFF, f.cutoff_hz), (F_RES, f.q)] {
                                if let Some(slot) = state.slot(id) {
                                    *slot = s.get(id).mapping.to_norm(value);
                                }
                                out.push(ParamEdit { param: id, value });
                            }
                        }
                        let pad = theme.sp(space::XS);
                        let inset = plot.shrink(pad);
                        overlay_menu(
                            ui,
                            theme,
                            s,
                            state,
                            F_MODE,
                            inset.left_top(),
                            egui::Align2::LEFT_TOP,
                            out,
                        );
                        overlay_menu(
                            ui,
                            theme,
                            s,
                            state,
                            F_SLOPE,
                            inset.left_top() + egui::vec2(theme.sp(space::XXL), 0.0),
                            egui::Align2::LEFT_TOP,
                            out,
                        );
                        overlay_menu(
                            ui,
                            theme,
                            s,
                            state,
                            F_POS,
                            inset.left_bottom()
                                + egui::vec2(theme.sp(control::POLY_MINI_VALUE_W), 0.0),
                            egui::Align2::LEFT_BOTTOM,
                            out,
                        );
                        overlay_slide(
                            ui,
                            theme,
                            s,
                            state,
                            F_DRIVE,
                            inset.left_bottom(),
                            egui::Align2::LEFT_BOTTOM,
                            "drv",
                            None,
                            "drive — drag; the chip beside it flips pre/post",
                            out,
                        );
                        overlay_slide(
                            ui,
                            theme,
                            s,
                            state,
                            F_KEY,
                            inset.right_bottom(),
                            egui::Align2::RIGHT_BOTTOM,
                            "key",
                            None,
                            "keytrack — drag; 100 % follows the keyboard",
                            out,
                        );
                    }
                    poly_widgets::CurveRegion::Header | poly_widgets::CurveRegion::Footer => {}
                }
            });
        },
    );

    ui.allocate_ui_with_layout(
        egui::vec2(width, envelope_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.set_height(envelope_h);
            poly_widgets::dark_curve_panel(ui, theme, None, envelope_plot_h, 0, 0, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => {
                        let plot = ui.max_rect();
                        edit_adsr(ui, theme, s, state, false, true, out);
                        // The sweep's DEPTH lives on the sweep's shape.
                        overlay_slide(
                            ui,
                            theme,
                            s,
                            state,
                            F_ENV,
                            plot.shrink(theme.sp(space::XS)).right_top(),
                            egui::Align2::RIGHT_TOP,
                            "env",
                            None,
                            "filter env amount — drag; bipolar",
                            out,
                        )
                    }
                    poly_widgets::CurveRegion::Header => {}
                    poly_widgets::CurveRegion::Footer => {}
                }
            });
        },
    );
}

fn amp_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Spec,
    state: &mut PolyUi,
    out: &mut Vec<ParamEdit>,
) {
    let width = ui.available_width();
    let plot_h = ui.available_height();

    ui.allocate_ui_with_layout(
        egui::vec2(width, plot_h),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_width(width);
            ui.set_height(plot_h);
            poly_widgets::dark_curve_panel(
                ui,
                theme,
                None,
                control::ENV_COMPACT_H,
                0,
                0,
                |ui, region| {
                    if region == poly_widgets::CurveRegion::Plot {
                        let plot = ui.max_rect().shrink(theme.sp(space::XS));
                        edit_adsr(ui, theme, s, state, true, true, out);
                        overlay_slide(
                            ui,
                            theme,
                            s,
                            state,
                            GAIN,
                            plot.right_top(),
                            egui::Align2::RIGHT_TOP,
                            "amp",
                            Some(&gain_db),
                            "gain — drag; double-click for unity",
                            out,
                        );
                        // VELOCITY as a needle dial, centre-right of the
                        // amp envelope: the amount velocity shapes the
                        // sound is a SWEEP, and its angle says "barely"
                        // or "fully" at a glance where a number does not.
                        let param = *s.get(VEL);
                        if let Some(norm) = state.slot(VEL)
                            && poly_widgets::dial_control(
                                ui,
                                theme,
                                egui::pos2(
                                    plot.right() - theme.sp(control::POLY_DIAL) * 0.6,
                                    plot.center().y,
                                ),
                                "amp-vel-dial",
                                "vel",
                                &param,
                                norm,
                                "velocity amount — drag; 0 % plays every note alike",
                            )
                        {
                            push_edit(s, state, VEL, out);
                        }
                    }
                },
            );
        },
    );
}

fn mod_panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Spec,
    state: &mut PolyUi,
    out: &mut Vec<ParamEdit>,
) {
    // A fixed three-rung routing field. Live routes keep their rung, the
    // first empty one becomes the invitation, and the remaining empty rungs
    // stay visible as quiet capacity. Opening MOD therefore feels like a
    // deliberate mode, never a blank remainder beneath the amp envelope.
    let gap = theme.sp(space::XS);
    let pad = theme.sp(space::SM);
    let rows = crate::params::poly::WIRES;
    let width = ui.available_width();
    let height = ui.available_height();
    let (well, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    {
        let painter = ui.painter();
        painter.rect_filled(
            well,
            crate::ui::device::design::screen_radius(),
            theme.surface_sunken,
        );
        painter.rect_stroke(
            well,
            crate::ui::device::design::screen_radius(),
            egui::Stroke::new(crate::ui::tokens::stroke::HAIR, theme.outline),
            egui::StrokeKind::Inside,
        );
    }
    let inner = well.shrink(pad);
    let row_h = theme.sp(control::POLY_CELL_H);
    let content_h = row_h * rows as f32 + gap * rows.saturating_sub(1) as f32;
    let content_top = inner.center().y - content_h * 0.5;
    let mut ghost_placed = false;
    for slot in 0..rows {
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(inner.left(), content_top + slot as f32 * (row_h + gap)),
            egui::vec2(inner.width(), row_h),
        );
        let mut row_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
        );
        row_ui.spacing_mut().item_spacing.x = gap;
        // Which wire, if any, owns this slot: live wires keep their own
        // rung so rows do not shuffle as others come and go.
        let (src, dst, amount) = crate::params::poly::WIRE_IDS[slot];
        let live = s.get(src).index(state.norm(src).unwrap_or_default()) != 0
            || s.get(dst).index(state.norm(dst).unwrap_or_default()) != 0;
        if live {
            let cell_w = ((inner.width() - gap * 2.0) / 3.0).max(0.0);
            for (id, menu) in [(src, true), (dst, true), (amount, false)] {
                row_ui.allocate_ui_with_layout(
                    egui::vec2(cell_w, row_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(cell_w);
                        let param = *s.get(id);
                        let Some(norm) = state.slot(id) else { return };
                        let moved = if menu {
                            poly_widgets::dropdown_cell(ui, theme, &param, norm)
                        } else {
                            poly_widgets::value_cell(ui, theme, &param, norm)
                        };
                        if moved {
                            push_edit(s, state, id, out);
                        }
                    },
                );
            }
        } else if !ghost_placed {
            ghost_placed = true;
            row_ui.allocate_ui_with_layout(
                egui::vec2(inner.width(), row_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let param = *s.get(src);
                    let Some(norm) = state.slot(src) else { return };
                    if poly_widgets::add_wire_cell(ui, theme, &param, norm) {
                        push_edit(s, state, src, out);
                    }
                },
            );
        }
        // A slot past the ghost stays an empty rung of the well.
    }
}

fn live_wire_count(s: &Spec, state: &PolyUi) -> usize {
    crate::params::poly::WIRE_IDS
        .iter()
        .filter(|&&(src, dst, _)| {
            s.get(src).index(state.norm(src).unwrap_or_default()) != 0
                || s.get(dst).index(state.norm(dst).unwrap_or_default()) != 0
        })
        .count()
}

/// Draw one fixed-size instrument whose named pages share one screen.
pub fn poly_card(ui: &mut egui::Ui, theme: &Theme, state: &mut PolyUi) -> Vec<ParamEdit> {
    // Keep only the oscillator bit and the two main-page bits. This clamps
    // corrupt or future state without changing any parameter.
    state.page &= 0b111;
    let s = spec();
    let mut edits = Vec::new();
    card::card_sized(ui, theme, "poly // voice", control::DEVICE_TALL_H, |ui| {
        let fp = |w| crate::ui::device::Footprint::new(theme.sp(w), 0.0);
        let layout = Wells::new()
            .compact()
            .row([Well::one().fits(fp(control::POLY_FACE_W)).filling()]);
        let face = ui.max_rect();
        card::wells(ui, theme, &layout, |ui, _| {
            ui.spacing_mut().item_spacing.y = theme.sp(space::XXS);
            let mut page = state.main_page();
            let badges = [0, 0, 0, live_wire_count(&s, state)];
            if poly_widgets::instrument_tabs(ui, theme, &mut page, badges) {
                state.show_main_page(page);
            }
            ui.push_id(("poly-page", page), |ui| match page {
                0 => oscillator_panel(ui, theme, &s, state, &mut edits),
                1 => filter_panel(ui, theme, &s, state, &mut edits),
                2 => amp_panel(ui, theme, &s, state, &mut edits),
                _ => mod_panel(ui, theme, &s, state, &mut edits),
            });
        });
        // LAST, over everything: the parameter under the hand, named and
        // numbered large enough to read. Nothing else on the card has to
        // spend permanent space on being legible at a glance.
        poly_widgets::touch_cartridge(ui, theme, face);
    });
    edits
}

// -------------------------------------------------------------- wiring ---

/// The natural value at a normalized knob position, by param id — the
/// mapping the card itself applies, reachable without drawing one.
pub fn poly_value(param: u32, norm: f32) -> f32 {
    spec().get(param).value(norm)
}

/// The inverse: where a NATURAL value sits on the knob. A device's stored
/// state is engine units, so this is the door back to what the card
/// draws, and it is the same `Param` in both directions — the round trip
/// cannot drift because there is only one mapping.
pub fn poly_norm(param: u32, value: f32) -> f32 {
    spec().get(param).mapping.to_norm(value)
}

/// Whether a parameter is a DISCRETE choice — a wave, a filter mode, an
/// octave — rather than a continuous value.
///
/// Worth exporting because a discrete parameter does NOT round-trip
/// through a knob position exactly, and is not meant to: the value that
/// comes back has snapped to the nearest segment. Anything checking the
/// round trip has to know which kind it is looking at, and this is the
/// card's own answer rather than a second list to keep in step.
pub fn poly_is_discrete(param: u32) -> bool {
    switch::is_discrete(spec().get(param))
}

/// An engine value in the words the card would print for it — "saw" for
/// a wave index, "1.05 s" for 1050 ms. The one formatter, shared, so a
/// parameter never reads differently in two places.
pub fn poly_format(param: u32, value: f32) -> String {
    let s = spec();
    let p = s.get(param);
    p.format(p.mapping.to_norm(value))
}

/// How many discrete choices a parameter has — 0 for a continuous one.
/// What a per-choice stepper needs to know, exported rather than
/// re-derived from the ranges.
pub fn poly_choices(param: u32) -> u32 {
    let s = spec();
    s.get(param).choices().unwrap_or(0)
}

/// Whether a parameter lives on a LOG scale — the card's own mapping,
/// asked rather than re-derived, so the modulation plan and the knob
/// cannot disagree about what kind of number this is.
pub fn poly_is_log(param: u32) -> bool {
    matches!(spec().get(param).mapping, param::Mapping::Log { .. })
}

/// Every parameter of `state` as an edit, whether or not it just changed.
/// What a caller needs after setting the knobs itself — a reset, a preset
/// recall, a project load — since the card only emits on user movement.
pub fn poly_edits(state: &PolyUi) -> Vec<ParamEdit> {
    let s = spec();
    TABLE
        .iter()
        .filter_map(|def| {
            state.norm(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: s.get(def.id).value(norm),
            })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn frame<R>(ctx: &egui::Context, f: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let mut f = Some(f);
        let mut out = None;
        let mut run = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1400.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                if let Some(f) = f.take() {
                    out = Some(f(ui));
                }
            },
        );
        run.textures_delta.clear();
        out.unwrap()
    }

    #[test]
    fn the_page_byte_keeps_local_and_main_navigation_independent() {
        let mut ui = PolyUi::default();
        for page in 0..4 {
            for osc in 0..=1 {
                ui.show_main_page(page);
                ui.show_oscillator(osc);
                assert_eq!(ui.main_page(), page);
                assert_eq!(ui.oscillator(), osc);
            }
        }
    }

    #[test]
    fn every_table_row_has_a_unit() {
        // `kind` is the one thing the table does not say. A row nobody
        // gave a unit to would silently fall through to a bare number.
        for def in TABLE {
            assert!(kind(def.id).is_some(), "{} has no unit", def.name);
        }
    }

    #[test]
    fn every_table_row_has_its_own_slot() {
        // THE map between the table and `PolyUi`. Two ids pointing at one
        // field is the copy-paste bug this whole id-list design exists to
        // make findable: the second write would clobber the first, and
        // one knob would move two parameters.
        let mut state = PolyUi::default();
        for (i, def) in TABLE.iter().enumerate() {
            let slot = state.slot(def.id);
            assert!(slot.is_some(), "{} has no slot", def.name);
            *slot.unwrap() = i as f32 / 100.0;
        }
        for (i, def) in TABLE.iter().enumerate() {
            assert_eq!(
                state.norm(def.id),
                Some(i as f32 / 100.0),
                "{} shares a slot with another parameter",
                def.name
            );
        }
        // An id the table does not have has no slot either.
        assert_eq!(state.slot(N as u32), None);
    }

    #[test]
    fn the_tabbed_instrument_accounts_for_every_parameter() {
        let mut reached = vec![
            A_WAVE, A_OCT, A_SEMI, A_FINE, A_LEVEL, A_PENV, B_WAVE, B_OCT, B_SEMI, B_FINE, B_LEVEL,
            B_PENV, N_COLOR, N_LEVEL, N_DECAY, F_MODE, F_SLOPE, F_CUTOFF, F_RES, F_ENV, F_KEY,
            F_DRIVE, F_POS, AMP_A, AMP_D, AMP_S, AMP_R, GAIN, VEL, V_MODE, V_GLIDE, V_UNISON,
            V_DETUNE, V_SPREAD, FENV_A, FENV_D, FENV_S, FENV_R, PENV_D,
        ];
        reached.extend(
            crate::params::poly::WIRE_IDS
                .into_iter()
                .flat_map(|ids| [ids.0, ids.1, ids.2]),
        );
        reached.sort_unstable();
        let before = reached.len();
        reached.dedup();
        assert_eq!(before, reached.len(), "a parameter is listed twice");
        assert_eq!(reached, TABLE.iter().map(|d| d.id).collect::<Vec<_>>());
    }

    #[test]
    fn a_choice_parameters_range_is_its_list_of_names() {
        // A table range and a name list that disagree is a switch with a
        // segment the engine will refuse, or a value the switch cannot
        // reach.
        for def in TABLE {
            if let Some(Kind::Choice(names)) = kind(def.id) {
                assert_eq!(def.min, 0.0, "{}", def.name);
                assert_eq!(
                    def.max,
                    names.len() as f32 - 1.0,
                    "{} has {} names for range 0..={}",
                    def.name,
                    names.len(),
                    def.max
                );
            }
        }
    }

    #[test]
    fn every_table_knob_leaves_as_an_edit() {
        // The widget must cover the table exactly: an id the table knows
        // but the card never emits is a knob that silently stopped
        // working. Same test the sine synth and reverb cards carry.
        let edits = poly_edits(&PolyUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect);
    }

    #[test]
    fn the_defaults_that_leave_are_the_tables_own() {
        // The card must open showing what the engine would do with a
        // freshly-defaulted patch — not an approximation of it.
        for edit in poly_edits(&PolyUi::default()) {
            let def = params::def(TABLE, edit.param);
            let tol = (def.max - def.min).abs() * 1e-3;
            assert!(
                (edit.value - def.default).abs() <= tol.max(1e-4),
                "{} left as {} but its default is {}",
                def.name,
                edit.value,
                def.default
            );
        }
    }

    #[test]
    fn defaults_are_one_audible_oscillator_and_nothing_else() {
        // The patch the card opens on has to make a sound the moment a
        // note arrives, and it has to be ONE sound — a default with both
        // oscillators and the noise source up is a patch the user starts
        // by subtracting from.
        let s = spec();
        let ui = PolyUi::default();
        let v = |id: u32| s.get(id).value(ui.norm(id).unwrap());
        assert!((v(A_LEVEL) - 100.0).abs() < 1e-3);
        assert_eq!(v(B_LEVEL), 0.0);
        assert_eq!(v(N_LEVEL), 0.0);
        assert!((v(GAIN) - 1.0).abs() < 1e-3);
        // The filter is parked open: loading the synth is not a filter
        // sweep nobody asked for.
        assert!(v(F_CUTOFF) > 15_000.0);
        // Neither oscillator is transposed or detuned.
        for ids in [
            [A_OCT, A_SEMI, A_FINE, A_PENV],
            [B_OCT, B_SEMI, B_FINE, B_PENV],
        ] {
            assert_eq!(
                v(ids[0]).round(),
                f32::from(crate::params::poly::OCT_CENTER as u8)
            );
            for id in &ids[1..] {
                assert!(v(*id).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn value_and_norm_are_inverses_for_every_parameter() {
        // The two doors onto one number. If these drifted, a project load
        // would land the knob somewhere other than where the value it
        // saved came from.
        //
        // A DISCRETE parameter is deliberately not an exact inverse:
        // `0.25` on an eight-way switch snaps to the nearest segment and
        // comes back as that segment's position. What must hold for it is
        // that snapping SETTLES — the second trip is a no-op — because a
        // value that crept one segment further on every save/load would
        // walk a patch across the whole list.
        for def in TABLE {
            let discrete = matches!(kind(def.id), Some(Kind::Choice(_)));
            for norm in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = poly_value(def.id, norm);
                let back = poly_norm(def.id, value);
                if discrete {
                    assert_eq!(
                        poly_value(def.id, back),
                        value,
                        "{} kept moving after it snapped",
                        def.name
                    );
                    assert!(
                        (value - value.round()).abs() < 1e-4,
                        "{} snapped to {value}, which is not a segment",
                        def.name
                    );
                } else {
                    assert!(
                        (back - norm).abs() < 1e-3,
                        "{} round-tripped {norm} to {back}",
                        def.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_curve_node_and_the_knobs_are_the_same_two_values() {
        // The response curve's draggable node and the cutoff/res knobs
        // edit one pair of numbers through one pair of mappings. If the
        // round trip drifted, dragging the node would nudge the knobs and
        // turning the knobs would nudge the node.
        let s = spec();
        let mut ui = PolyUi::default();
        for (cutoff, res) in [(0.0f32, 0.0f32), (0.37, 0.5), (1.0, 1.0)] {
            ui.cutoff = cutoff;
            ui.res = res;
            let f = ui.filter(&s);
            let back_cutoff = s.get(F_CUTOFF).mapping.to_norm(f.cutoff_hz);
            let back_res = s.get(F_RES).mapping.to_norm(f.q);
            assert!((back_cutoff - cutoff).abs() < 1e-3, "cutoff {cutoff}");
            assert!((back_res - res).abs() < 1e-3, "res {res}");
        }
    }

    #[test]
    fn discrete_params_get_the_switch_and_continuous_ones_get_the_knob() {
        // The section grammar's load-bearing assumption: a well lists its
        // ids in order and never says which widget each one is.
        let s = spec();
        for def in TABLE {
            let discrete = switch::is_discrete(s.get(def.id));
            let is_choice = matches!(kind(def.id), Some(Kind::Choice(_)));
            assert_eq!(discrete, is_choice, "{} picked the wrong widget", def.name);
        }
    }

    #[test]
    fn only_cents_and_linear_gain_show_a_bare_number() {
        // "Never show 0..1" — every readout is in the unit the parameter
        // is actually in. `Plain` is allowed exactly where `Unit` has no
        // arm yet: cents, and a linear gain that matches `seq::GAIN`.
        let s = spec();
        let plain: Vec<&str> = TABLE
            .iter()
            .filter(|d| s.get(d.id).unit == param::Unit::Plain)
            .map(|d| d.name)
            .collect();
        assert_eq!(plain, ["a fine", "b fine", "filter res", "amp gain"]);
    }

    #[test]
    fn every_instrument_page_draws_without_emitting_at_rest() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        // All four main pages and both oscillator selections draw clean.
        // Bits above the navigation field clamp away on the first draw.
        for page in 0..8 {
            let mut state = PolyUi {
                page: page | 0b1000,
                ..PolyUi::default()
            };
            let mut edits = Vec::new();
            for _ in 0..3 {
                edits = frame(&ctx, |ui| {
                    egui::CentralPanel::default()
                        .show(ui, |ui| poly_card(ui, &theme, &mut state))
                        .inner
                });
            }
            assert!(edits.is_empty(), "page {page} emitted {edits:?} at rest");
            let want = PolyUi {
                page,
                ..PolyUi::default()
            };
            assert_eq!(state, want, "page {page} moved a control by drawing");
        }
    }

    #[test]
    fn the_tabbed_instrument_stays_inside_its_width_budget() {
        const BUDGET: f32 = 520.0;
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = PolyUi::default();
        let w = frame(&ctx, |ui| {
            egui::CentralPanel::default()
                .show(ui, |ui| {
                    ui.horizontal(|ui| poly_card(ui, &theme, &mut state))
                        .response
                        .rect
                        .width()
                })
                .inner
        });
        assert!(w <= BUDGET, "face is {w:.0} pt wide, over {BUDGET:.0}");
        assert!(w > 470.0, "the tabbed face did not draw ({w:.0} pt)");
    }
}
