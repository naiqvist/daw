//! LOOM's device card — the wavetable synth.
//!
//! Five pages behind one tab row: two oscillators with the WAVETABLE MAP
//! as the hero (every table in the set drawn stacked, the morph position
//! marking where the knob is), then noise + filter, amp, and voice. The
//! section grammar repeats: title, visual, then knobs — learning one
//! page teaches all of them.
//!
//! The card owns no state the instance cannot hand back: `page` rides
//! `DeviceInstance` like every paged instrument's does, and every other
//! field is a knob position rebuilt from engine units each frame.

use crate::params::loom as lp;
use crate::params::{self};
use crate::ui::device::adjust;
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{Footprint, Param, Well, Wells, card, metrics, poly_widgets};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

const CARD_H: f32 = control::DEVICE_TALL_H;
const CELL_UNITS: usize = 2;
const PAGE_BITS: usize = 3;
const PAGE_MASK: usize = (1 << PAGE_BITS) - 1;
const TAB_LABELS: [&str; 5] = ["OSC A", "OSC B", "NOISE+FLT", "AMP", "VOICE"];
const FACE_ROWS: [&[u32]; 14] = [
    &[lp::A_MORPH, lp::A_OCT, lp::A_SEMI, lp::A_LEVEL],
    &[lp::B_MORPH, lp::B_OCT, lp::B_SEMI, lp::B_LEVEL],
    &[lp::F_MODE, lp::F_CUTOFF],
    &[lp::F_RES, lp::F_ENV],
    &[lp::N_LEVEL, lp::N_DECAY],
    &[lp::GAIN, lp::VEL],
    &[lp::AMP_A, lp::AMP_D],
    &[lp::AMP_S, lp::AMP_R],
    &[lp::FENV_A, lp::FENV_D],
    &[lp::FENV_S, lp::FENV_R],
    &[lp::V_UNISON, lp::V_DETUNE],
    &[lp::V_SPREAD, lp::V_GLIDE],
    &[lp::LFO_RATE, lp::LFO_PITCH],
    &[lp::PENV, lp::PENV_D],
];
/// How many waves the map draws, and the analysis shape shared with the
/// display.
const WAVES: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LoomUi {
    /// Which page is open — persisted on the instance, like poly's.
    pub page: usize,
    /// The preset stepper's own position — view state, like the page.
    pub preset_index: f32,
    pub a_morph: f32,
    pub a_oct: f32,
    pub a_semi: f32,
    pub a_level: f32,
    pub b_morph: f32,
    pub b_oct: f32,
    pub b_semi: f32,
    pub b_level: f32,
    pub n_level: f32,
    pub n_decay: f32,
    pub f_mode: f32,
    pub f_cutoff: f32,
    pub f_res: f32,
    pub f_env: f32,
    pub amp_a: f32,
    pub amp_d: f32,
    pub amp_s: f32,
    pub amp_r: f32,
    pub gain: f32,
    pub vel: f32,
    pub fenv_a: f32,
    pub fenv_d: f32,
    pub fenv_s: f32,
    pub fenv_r: f32,
    pub v_unison: f32,
    pub v_detune: f32,
    pub v_spread: f32,
    pub v_glide: f32,
    pub penv_d: f32,
    pub penv: f32,
    pub lfo_rate: f32,
    pub lfo_pitch: f32,
}

impl Default for LoomUi {
    fn default() -> Self {
        let at = |id: u32| loom_norm(id, params::def(lp::TABLE, id).default);
        Self {
            page: 0,
            preset_index: 0.0,
            a_morph: at(lp::A_MORPH),
            a_oct: at(lp::A_OCT),
            a_semi: at(lp::A_SEMI),
            a_level: at(lp::A_LEVEL),
            b_morph: at(lp::B_MORPH),
            b_oct: at(lp::B_OCT),
            b_semi: at(lp::B_SEMI),
            b_level: at(lp::B_LEVEL),
            n_level: at(lp::N_LEVEL),
            n_decay: at(lp::N_DECAY),
            f_mode: at(lp::F_MODE),
            f_cutoff: at(lp::F_CUTOFF),
            f_res: at(lp::F_RES),
            f_env: at(lp::F_ENV),
            amp_a: at(lp::AMP_A),
            amp_d: at(lp::AMP_D),
            amp_s: at(lp::AMP_S),
            amp_r: at(lp::AMP_R),
            gain: at(lp::GAIN),
            vel: at(lp::VEL),
            fenv_a: at(lp::FENV_A),
            fenv_d: at(lp::FENV_D),
            fenv_s: at(lp::FENV_S),
            fenv_r: at(lp::FENV_R),
            v_unison: at(lp::V_UNISON),
            v_detune: at(lp::V_DETUNE),
            v_spread: at(lp::V_SPREAD),
            v_glide: at(lp::V_GLIDE),
            penv_d: at(lp::PENV_D),
            penv: at(lp::PENV),
            lfo_rate: at(lp::LFO_RATE),
            lfo_pitch: at(lp::LFO_PITCH),
        }
    }
}

impl LoomUi {
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| loom_norm(id, get(id));
        Self {
            page: 0,
            preset_index: 0.0,
            a_morph: at(lp::A_MORPH),
            a_oct: at(lp::A_OCT),
            a_semi: at(lp::A_SEMI),
            a_level: at(lp::A_LEVEL),
            b_morph: at(lp::B_MORPH),
            b_oct: at(lp::B_OCT),
            b_semi: at(lp::B_SEMI),
            b_level: at(lp::B_LEVEL),
            n_level: at(lp::N_LEVEL),
            n_decay: at(lp::N_DECAY),
            f_mode: at(lp::F_MODE),
            f_cutoff: at(lp::F_CUTOFF),
            f_res: at(lp::F_RES),
            f_env: at(lp::F_ENV),
            amp_a: at(lp::AMP_A),
            amp_d: at(lp::AMP_D),
            amp_s: at(lp::AMP_S),
            amp_r: at(lp::AMP_R),
            gain: at(lp::GAIN),
            vel: at(lp::VEL),
            fenv_a: at(lp::FENV_A),
            fenv_d: at(lp::FENV_D),
            fenv_s: at(lp::FENV_S),
            fenv_r: at(lp::FENV_R),
            v_unison: at(lp::V_UNISON),
            v_detune: at(lp::V_DETUNE),
            v_spread: at(lp::V_SPREAD),
            v_glide: at(lp::V_GLIDE),
            penv_d: at(lp::PENV_D),
            penv: at(lp::PENV),
            lfo_rate: at(lp::LFO_RATE),
            lfo_pitch: at(lp::LFO_PITCH),
        }
    }

    /// Restore Loom's non-engine view state from `DeviceInstance::page`.
    pub fn restore_view(&mut self, packed: u8) {
        self.page = usize::from(packed) & PAGE_MASK;
        let preset = (usize::from(packed) >> PAGE_BITS).min(PRESET_NAMES.len() - 1);
        self.preset_index = preset_param().at_index(preset);
    }

    /// Pack the page and preset position into `DeviceInstance::page`.
    pub fn packed_view(&self) -> u8 {
        let page = self.page.min(TAB_LABELS.len() - 1);
        let preset = preset_param()
            .index(self.preset_index)
            .min(PRESET_NAMES.len() - 1);
        ((preset << PAGE_BITS) | page).min(u8::MAX as usize) as u8
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            lp::A_MORPH => &mut self.a_morph,
            lp::A_OCT => &mut self.a_oct,
            lp::A_SEMI => &mut self.a_semi,
            lp::A_LEVEL => &mut self.a_level,
            lp::B_MORPH => &mut self.b_morph,
            lp::B_OCT => &mut self.b_oct,
            lp::B_SEMI => &mut self.b_semi,
            lp::B_LEVEL => &mut self.b_level,
            lp::N_LEVEL => &mut self.n_level,
            lp::N_DECAY => &mut self.n_decay,
            lp::F_MODE => &mut self.f_mode,
            lp::F_CUTOFF => &mut self.f_cutoff,
            lp::F_RES => &mut self.f_res,
            lp::F_ENV => &mut self.f_env,
            lp::AMP_A => &mut self.amp_a,
            lp::AMP_D => &mut self.amp_d,
            lp::AMP_S => &mut self.amp_s,
            lp::AMP_R => &mut self.amp_r,
            lp::GAIN => &mut self.gain,
            lp::VEL => &mut self.vel,
            lp::FENV_A => &mut self.fenv_a,
            lp::FENV_D => &mut self.fenv_d,
            lp::FENV_S => &mut self.fenv_s,
            lp::FENV_R => &mut self.fenv_r,
            lp::V_UNISON => &mut self.v_unison,
            lp::V_DETUNE => &mut self.v_detune,
            lp::V_SPREAD => &mut self.v_spread,
            lp::V_GLIDE => &mut self.v_glide,
            lp::PENV_D => &mut self.penv_d,
            lp::PENV => &mut self.penv,
            lp::LFO_RATE => &mut self.lfo_rate,
            lp::LFO_PITCH => &mut self.lfo_pitch,
            _ => return None,
        })
    }

    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }

    #[cfg(test)]
    fn norm(&self, param: u32) -> Option<f32> {
        Some(match param {
            lp::A_MORPH => self.a_morph,
            lp::A_OCT => self.a_oct,
            lp::A_SEMI => self.a_semi,
            lp::A_LEVEL => self.a_level,
            lp::B_MORPH => self.b_morph,
            lp::B_OCT => self.b_oct,
            lp::B_SEMI => self.b_semi,
            lp::B_LEVEL => self.b_level,
            lp::N_LEVEL => self.n_level,
            lp::N_DECAY => self.n_decay,
            lp::F_MODE => self.f_mode,
            lp::F_CUTOFF => self.f_cutoff,
            lp::F_RES => self.f_res,
            lp::F_ENV => self.f_env,
            lp::AMP_A => self.amp_a,
            lp::AMP_D => self.amp_d,
            lp::AMP_S => self.amp_s,
            lp::AMP_R => self.amp_r,
            lp::GAIN => self.gain,
            lp::VEL => self.vel,
            lp::FENV_A => self.fenv_a,
            lp::FENV_D => self.fenv_d,
            lp::FENV_S => self.fenv_s,
            lp::FENV_R => self.fenv_r,
            lp::V_UNISON => self.v_unison,
            lp::V_DETUNE => self.v_detune,
            lp::V_SPREAD => self.v_spread,
            lp::V_GLIDE => self.v_glide,
            lp::PENV_D => self.penv_d,
            lp::PENV => self.penv,
            lp::LFO_RATE => self.lfo_rate,
            lp::LFO_PITCH => self.lfo_pitch,
            _ => return None,
        })
    }
}

fn param_of(id: u32) -> Param {
    let def = params::def(lp::TABLE, id);
    let with = |p: Param| p.with_default(shown(id, def.default));
    match id {
        lp::A_OCT | lp::B_OCT => with(Param::choice("octave", lp::OCTAVES)),
        lp::F_MODE => with(Param::choice("mode", lp::FILTER_MODES)),
        lp::V_UNISON => with(Param::choice("unison", lp::UNISON)),
        lp::A_SEMI | lp::B_SEMI => with(Param::new(
            "semi",
            crate::ui::device::param::Mapping::Linear {
                min: -12.0,
                max: 12.0,
            },
            crate::ui::device::param::Unit::Semitones,
        )),
        lp::F_ENV => with(Param::new(
            "f env",
            crate::ui::device::param::Mapping::Linear {
                min: -1.0,
                max: 1.0,
            },
            crate::ui::device::param::Unit::Plain,
        )),
        lp::F_CUTOFF => with(Param::hz("cutoff", def.min, def.max)),
        lp::AMP_A
        | lp::AMP_D
        | lp::AMP_R
        | lp::FENV_A
        | lp::FENV_D
        | lp::FENV_R
        | lp::N_DECAY
        | lp::V_GLIDE
        | lp::PENV_D => with(Param::ms(id_name(id), def.min, def.max)),
        lp::PENV => with(Param::new(
            "p env",
            crate::ui::device::param::Mapping::Linear {
                min: -48.0,
                max: 48.0,
            },
            crate::ui::device::param::Unit::Semitones,
        )),
        lp::LFO_RATE => with(Param::hz("lfo rate", def.min, def.max)),
        lp::LFO_PITCH => with(Param::new(
            "lfo pitch",
            crate::ui::device::param::Mapping::Linear { min: 0.0, max: 2.0 },
            crate::ui::device::param::Unit::Plain,
        )),
        lp::AMP_S | lp::FENV_S => with(Param::percent("sustain")),
        lp::A_MORPH
        | lp::B_MORPH
        | lp::A_LEVEL
        | lp::B_LEVEL
        | lp::N_LEVEL
        | lp::F_RES
        | lp::VEL
        | lp::V_DETUNE
        | lp::V_SPREAD
        | lp::GAIN => {
            let name = id_name(id);
            if id == lp::GAIN {
                with(Param::new(
                    name,
                    crate::ui::device::param::Mapping::Linear { min: 0.0, max: 2.0 },
                    crate::ui::device::param::Unit::Ratio,
                ))
            } else {
                with(Param::percent(name))
            }
        }
        _ => with(Param::percent(id_name(id))),
    }
}

fn id_name(id: u32) -> &'static str {
    params::def(lp::TABLE, id).name
}

fn shown(param: u32, value: f32) -> f32 {
    if fraction_as_percent(param) {
        value * 100.0
    } else {
        value
    }
}

fn natural(param: u32, value: f32) -> f32 {
    let value = if fraction_as_percent(param) {
        value / 100.0
    } else {
        value
    };
    params::def(lp::TABLE, param).clamp(value)
}

fn fraction_as_percent(param: u32) -> bool {
    matches!(param, lp::A_MORPH | lp::B_MORPH | lp::AMP_S | lp::FENV_S)
}

pub fn loom_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn loom_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

pub fn loom_is_discrete(param: u32) -> bool {
    param_of(param).choices().is_some()
}

pub fn loom_is_log(param: u32) -> bool {
    matches!(
        param_of(param).mapping,
        crate::ui::device::param::Mapping::Log { .. }
    )
}

/// One table's waveform, drawn analytically — a DRAWING of the shape,
/// not the kernel's output. Seven waves, the map's whole vocabulary.
pub fn loom_wave(shape: usize, phase: f32) -> f32 {
    let t = phase.rem_euclid(1.0);
    match shape {
        0 => (core::f32::consts::TAU * t).sin(),
        1 => 1.0 - 4.0 * (t - 0.5).abs(),
        2 => 2.0 * t - 1.0,
        3 => {
            if t < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        4 => {
            // Bell: partials 1, 2, 5, 9, 13 — a few, spread wide.
            let mut v = 0.0f32;
            for (n, amp) in [(1, 1.0), (2, 0.7), (5, 0.45), (9, 0.3), (13, 0.2)] {
                v += (core::f32::consts::TAU * t * n as f32).sin() * amp;
            }
            v * 0.4
        }
        5 => {
            // Glass: odd partials rolling off, bright and thin.
            let mut v = 0.0f32;
            for n in (1..=31).step_by(2) {
                v += (core::f32::consts::TAU * t * n as f32).sin() / (n as f32).sqrt();
            }
            v * 0.25
        }
        _ => {
            // Metal: every partial, 1/n, dense and clangorous.
            let mut v = 0.0f32;
            for n in 1..=32 {
                let sign = if (n / 2) % 2 == 0 { 1.0 } else { -1.0 };
                v += (core::f32::consts::TAU * t * n as f32).sin() * sign / n as f32;
            }
            // 0.3 keeps the clangorous sum inside the rails — the true
            // peak of the partial stack is 1.36, and a wave drawn
            // clipped would lie about what the oscillator holds.
            v * 0.3
        }
    }
}

/// The five tabs, poly's row shape with loom's labels.
fn loom_tabs(ui: &mut egui::Ui, theme: &Theme, selected: &mut usize) {
    let size = egui::vec2(ui.available_width(), theme.sp(control::POLY_TAB_H));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    *selected = (*selected).min(TAB_LABELS.len() - 1);
    let cell_w = rect.width() / TAB_LABELS.len() as f32;
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        *selected = (((pos.x - rect.left()) / cell_w) as usize).min(TAB_LABELS.len() - 1);
        response.request_focus();
    }
    let step = adjust::steps(ui, &response);
    if step != 0 {
        *selected = (*selected as i32 + step).rem_euclid(TAB_LABELS.len() as i32) as usize;
    }

    // Poly's signal-path grammar: numbered stations, a connecting rail,
    // and a family colour for the section currently under the hand.
    let bright = [
        theme.role_shape,
        theme.role_shape,
        theme.role_time,
        theme.role_level,
        theme.role_mod,
    ];
    let dim = [
        theme.role_shape_dim,
        theme.role_shape_dim,
        theme.role_time_dim,
        theme.role_level_dim,
        theme.role_mod_dim,
    ];
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        crate::ui::device::design::screen_radius(),
        theme.surface_sunken,
    );
    painter.rect_stroke(
        rect,
        crate::ui::device::design::screen_radius(),
        egui::Stroke::new(stroke::HAIR, theme.outline),
        egui::StrokeKind::Inside,
    );
    let rail_y = rect.bottom() - theme.sp(space::XS);
    painter.line_segment(
        [
            egui::pos2(rect.left() + cell_w * 0.5, rail_y),
            egui::pos2(rect.right() - cell_w * 0.5, rail_y),
        ],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    for (i, label) in TAB_LABELS.iter().enumerate() {
        let tab = egui::Rect::from_min_size(
            rect.min + egui::vec2(cell_w * i as f32, 0.0),
            egui::vec2(cell_w, rect.height()),
        );
        if i > 0 {
            painter.vline(
                tab.left(),
                tab.y_range(),
                egui::Stroke::new(stroke::HAIR, theme.divider),
            );
        }
        let on = i == *selected;
        if on {
            painter.rect_filled(
                tab.shrink(stroke::HAIR),
                crate::ui::device::design::screen_radius(),
                dim[i].gamma_multiply(0.72),
            );
        }
        painter.circle_filled(
            egui::pos2(tab.center().x, rail_y),
            theme.sp(if on { space::XXS } else { stroke::HAIR }),
            if on { bright[i] } else { theme.outline },
        );
        painter.text(
            egui::pos2(tab.left() + theme.sp(space::XS), tab.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{:02}", i + 1),
            egui::FontId::monospace(font::MICRO_LABEL),
            if on { bright[i] } else { theme.outline },
        );
        painter.text(
            tab.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Proportional),
            if on { bright[i] } else { theme.text_muted },
        );
        if on {
            painter.hline(
                tab.x_range(),
                tab.bottom() - stroke::HAIR,
                egui::Stroke::new(stroke::BOLD, bright[i]),
            );
        }
    }
    if response.has_focus() {
        crate::ui::device::design::focus_ring(painter, theme, rect);
    }
    response.on_hover_text("instrument page — click, wheel, or use arrow keys");
}

pub fn loom_edits(state: &LoomUi) -> Vec<ParamEdit> {
    let mut state = *state;
    lp::TABLE
        .iter()
        .filter_map(|def| {
            state.slot(def.id).map(|norm| ParamEdit {
                param: def.id,
                value: loom_value(def.id, *norm),
            })
        })
        .collect()
}

fn cell_min_width(ui: &egui::Ui, theme: &Theme, p: &Param) -> f32 {
    let value = metrics::mono_w(ui, &p.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &p.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

pub fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    // The widest row on ANY page decides the face. In particular, the
    // oscillator's four cells need more room than a representative pair;
    // sizing from a pair is how A MORPH used to print through OCTAVE.
    let gap = ui.spacing().item_spacing.x;
    let cells = FACE_ROWS
        .iter()
        .map(|row| {
            row.iter()
                .map(|id| cell_min_width(ui, theme, &param_of(*id)))
                .sum::<f32>()
                + gap * row.len().saturating_sub(1) as f32
        })
        .fold(0.0f32, f32::max);
    let tabs = TAB_LABELS
        .iter()
        .map(|label| metrics::text_w(ui, label, font::MINI_LABEL) + theme.sp(space::XS) * 2.0)
        .sum::<f32>();
    cells.max(tabs).max(theme.sp(control::POLY_FACE_W))
}

fn face(ui: &egui::Ui, theme: &Theme) -> Wells {
    Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()])
}

/// The presets — the front door, and the workhorse promise: hats, snares,
/// basses, dub sirens, pads and keys come OUT of the box, not as a
/// sound-design homework assignment. One row of natural values per
/// preset, applied onto the table defaults.
pub const PRESET_NAMES: &[&str] = &[
    "sub bass",
    "kick",
    "snare",
    "hat closed",
    "hat open",
    "clap",
    "dub siren",
    "pad",
    "keys",
    "lead",
    "pluck",
];

fn preset_param() -> Param {
    Param::choice("preset", PRESET_NAMES)
}

/// One preset's overrides: `(param id, natural value)`. Everything not
/// named keeps its table default.
fn preset_rows(index: usize) -> &'static [(u32, f32)] {
    match index {
        0 => &[
            (lp::A_MORPH, 0.0),
            (lp::A_OCT, 3.0),
            (lp::A_LEVEL, 100.0),
            (lp::B_LEVEL, 0.0),
            (lp::F_CUTOFF, 300.0),
            (lp::AMP_A, 4.0),
            (lp::AMP_D, 400.0),
            (lp::AMP_S, 0.8),
            (lp::AMP_R, 200.0),
            (lp::GAIN, 0.9),
            (lp::VEL, 60.0),
            (lp::V_GLIDE, 20.0),
        ],
        // The kick: a sine falling +48 semitones over its first breath,
        // a one-sample click of noise, a soft-clipped bottom.
        1 => &[
            (lp::A_MORPH, 0.0),
            (lp::A_OCT, 3.0),
            (lp::A_LEVEL, 100.0),
            (lp::B_LEVEL, 0.0),
            (lp::N_LEVEL, 25.0),
            (lp::N_DECAY, 3.0),
            (lp::F_CUTOFF, 900.0),
            (lp::F_ENV, 0.8),
            (lp::AMP_A, 0.5),
            (lp::AMP_D, 250.0),
            (lp::AMP_S, 0.0),
            (lp::AMP_R, 40.0),
            (lp::FENV_A, 0.5),
            (lp::FENV_D, 120.0),
            (lp::FENV_S, 0.0),
            (lp::FENV_R, 30.0),
            (lp::PENV_D, 60.0),
            (lp::PENV, 48.0),
            (lp::GAIN, 1.0),
        ],
        // The snare: noise through a bandpass, a triangle cracking down
        // two octaves under it.
        2 => &[
            (lp::A_MORPH, 0.1667),
            (lp::A_OCT, 3.0),
            (lp::A_LEVEL, 60.0),
            (lp::B_MORPH, 0.1667),
            (lp::B_LEVEL, 60.0),
            (lp::N_LEVEL, 90.0),
            (lp::N_DECAY, 180.0),
            (lp::F_MODE, 2.0),
            (lp::F_CUTOFF, 900.0),
            (lp::F_RES, 20.0),
            (lp::F_ENV, 0.5),
            (lp::AMP_A, 0.5),
            (lp::AMP_D, 140.0),
            (lp::AMP_S, 0.0),
            (lp::AMP_R, 60.0),
            (lp::PENV_D, 30.0),
            (lp::PENV, -24.0),
            (lp::GAIN, 0.9),
        ],
        3 => &[
            (lp::A_LEVEL, 0.0),
            (lp::B_LEVEL, 0.0),
            (lp::N_LEVEL, 100.0),
            (lp::N_DECAY, 35.0),
            (lp::F_MODE, 1.0),
            (lp::F_CUTOFF, 7_000.0),
            (lp::F_ENV, 0.6),
            (lp::AMP_A, 0.3),
            (lp::AMP_D, 40.0),
            (lp::AMP_S, 0.0),
            (lp::AMP_R, 10.0),
            (lp::FENV_A, 0.3),
            (lp::FENV_D, 60.0),
            (lp::FENV_S, 0.0),
            (lp::FENV_R, 10.0),
            (lp::GAIN, 0.7),
            (lp::VEL, 90.0),
        ],
        4 => &[
            (lp::A_LEVEL, 0.0),
            (lp::B_LEVEL, 0.0),
            (lp::N_LEVEL, 100.0),
            (lp::N_DECAY, 300.0),
            (lp::F_MODE, 1.0),
            (lp::F_CUTOFF, 7_000.0),
            (lp::F_ENV, 0.5),
            (lp::AMP_A, 0.3),
            (lp::AMP_D, 200.0),
            (lp::AMP_S, 0.0),
            (lp::AMP_R, 100.0),
            (lp::FENV_A, 0.3),
            (lp::FENV_D, 120.0),
            (lp::FENV_S, 0.0),
            (lp::FENV_R, 50.0),
            (lp::GAIN, 0.7),
            (lp::VEL, 90.0),
        ],
        5 => &[
            (lp::A_LEVEL, 0.0),
            (lp::B_LEVEL, 0.0),
            (lp::N_LEVEL, 95.0),
            (lp::N_DECAY, 140.0),
            (lp::F_MODE, 2.0),
            (lp::F_CUTOFF, 1_200.0),
            (lp::F_RES, 25.0),
            (lp::F_ENV, 0.4),
            (lp::AMP_A, 0.5),
            (lp::AMP_D, 110.0),
            (lp::AMP_S, 0.0),
            (lp::AMP_R, 50.0),
            (lp::GAIN, 0.8),
        ],
        // The dub siren: a saw bending ±1.6 octaves on the internal
        // LFO, with glide between the two notes the hand alternates.
        6 => &[
            (lp::A_MORPH, 0.35),
            (lp::A_LEVEL, 100.0),
            (lp::B_LEVEL, 0.0),
            (lp::F_CUTOFF, 5_000.0),
            (lp::F_RES, 25.0),
            (lp::F_ENV, 0.3),
            (lp::AMP_A, 3.0),
            (lp::AMP_D, 200.0),
            (lp::AMP_S, 0.9),
            (lp::AMP_R, 150.0),
            (lp::V_GLIDE, 120.0),
            (lp::LFO_RATE, 3.5),
            (lp::LFO_PITCH, 1.6),
            (lp::GAIN, 0.8),
            (lp::VEL, 90.0),
        ],
        // The pad: four detuned voices, slow attacks, a whisper of
        // LFO on the pitch so the ensemble breathes.
        7 => &[
            (lp::A_MORPH, 0.35),
            (lp::A_LEVEL, 80.0),
            (lp::B_MORPH, 0.2),
            (lp::B_SEMI, 7.0),
            (lp::B_LEVEL, 60.0),
            (lp::F_CUTOFF, 2_200.0),
            (lp::F_RES, 20.0),
            (lp::F_ENV, 0.4),
            (lp::AMP_A, 350.0),
            (lp::AMP_D, 600.0),
            (lp::AMP_S, 0.8),
            (lp::AMP_R, 800.0),
            (lp::FENV_A, 300.0),
            (lp::FENV_D, 500.0),
            (lp::FENV_S, 0.3),
            (lp::FENV_R, 400.0),
            (lp::V_UNISON, 3.0),
            (lp::V_DETUNE, 20.0),
            (lp::V_SPREAD, 100.0),
            (lp::LFO_RATE, 0.3),
            (lp::LFO_PITCH, 0.05),
            (lp::GAIN, 0.5),
            (lp::VEL, 70.0),
        ],
        8 => &[
            (lp::A_MORPH, 0.4),
            (lp::A_LEVEL, 90.0),
            (lp::B_MORPH, 0.2),
            (lp::B_OCT, 5.0),
            (lp::B_LEVEL, 45.0),
            (lp::F_CUTOFF, 4_000.0),
            (lp::F_ENV, 0.5),
            (lp::AMP_A, 2.0),
            (lp::AMP_D, 500.0),
            (lp::AMP_S, 0.5),
            (lp::AMP_R, 200.0),
            (lp::FENV_A, 2.0),
            (lp::FENV_D, 300.0),
            (lp::FENV_S, 0.2),
            (lp::FENV_R, 100.0),
            (lp::GAIN, 0.7),
            (lp::VEL, 90.0),
        ],
        9 => &[
            (lp::A_MORPH, 0.35),
            (lp::A_LEVEL, 100.0),
            (lp::B_MORPH, 0.35),
            (lp::B_SEMI, 12.0),
            (lp::B_LEVEL, 50.0),
            (lp::F_CUTOFF, 7_000.0),
            (lp::F_ENV, 0.3),
            (lp::AMP_A, 2.0),
            (lp::AMP_D, 250.0),
            (lp::AMP_S, 0.8),
            (lp::AMP_R, 180.0),
            (lp::V_GLIDE, 60.0),
            (lp::GAIN, 0.8),
        ],
        _ => &[
            (lp::A_MORPH, 0.35),
            (lp::A_LEVEL, 100.0),
            (lp::B_LEVEL, 0.0),
            (lp::F_CUTOFF, 3_500.0),
            (lp::F_RES, 20.0),
            (lp::F_ENV, 1.0),
            (lp::AMP_A, 0.5),
            (lp::AMP_D, 300.0),
            (lp::AMP_S, 0.0),
            (lp::AMP_R, 150.0),
            (lp::FENV_A, 0.5),
            (lp::FENV_D, 220.0),
            (lp::FENV_S, 0.0),
            (lp::FENV_R, 80.0),
            (lp::GAIN, 0.8),
        ],
    }
}

/// Load one preset into the state: defaults first, then the row's
/// overrides, and every parameter comes back out as an edit — the
/// letters are what make the engine follow.
fn load_preset(state: &mut LoomUi, index: usize) -> Vec<ParamEdit> {
    let page = state.page;
    let index = index.min(PRESET_NAMES.len() - 1);
    *state = LoomUi {
        page,
        preset_index: preset_param().at_index(index),
        ..LoomUi::default()
    };
    for (param, value) in preset_rows(index) {
        let norm = loom_norm(*param, *value).clamp(0.0, 1.0);
        state.set_norm(*param, norm);
    }
    loom_edits(state)
}

/// The front door: one stepper over the preset names, in the OSC A
/// page's header. Choosing one loads it and emits the whole table.
fn preset_stepper(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut LoomUi,
    edits: &mut Vec<ParamEdit>,
) {
    let param = preset_param();
    let mut at = state.preset_index;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ui.available_height()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            if poly_widgets::labeled_cell_steps(ui, theme, &param, &mut at, None) {
                let loaded = load_preset(state, param.index(at));
                edits.extend(loaded);
            }
        },
    );
}

/// Draw the generator's card. Returns the edits the user made.
pub fn loom_card(ui: &mut egui::Ui, theme: &Theme, state: &mut LoomUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = face(ui, theme);
    card::card_sized(ui, theme, "loom // wavetable", CARD_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            ui.spacing_mut().item_spacing.y = theme.sp(space::XXS);
            let mut page = state.page;
            loom_tabs(ui, theme, &mut page);
            state.page = page;
            ui.push_id(("loom-page", page), |ui| match page {
                0 => osc_page(ui, theme, state, &mut edits, true),
                1 => osc_page(ui, theme, state, &mut edits, false),
                2 => noise_filter_page(ui, theme, state, &mut edits),
                3 => amp_page(ui, theme, state, &mut edits),
                _ => voice_page(ui, theme, state, &mut edits),
            });
        });
    });
    edits
}

/// One oscillator page: four cells under the wavetable map. `is_a` picks
/// the knob set.
fn osc_page(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut LoomUi,
    edits: &mut Vec<ParamEdit>,
    is_a: bool,
) {
    let ids: [u32; 4] = if is_a {
        [lp::A_MORPH, lp::A_OCT, lp::A_SEMI, lp::A_LEVEL]
    } else {
        [lp::B_MORPH, lp::B_OCT, lp::B_SEMI, lp::B_LEVEL]
    };
    let morph = if is_a { state.a_morph } else { state.b_morph };
    let footer_units = CELL_UNITS;
    // The PRESETS are the front door, and the front door is the first
    // oscillator page: one stepper in the header, the map under it.
    let header_units = if is_a { 2 } else { 0 };
    poly_widgets::dark_curve_panel(
        ui,
        theme,
        None,
        0.0,
        header_units,
        footer_units,
        |ui, region| match region {
            poly_widgets::CurveRegion::Plot => wavetable_map(ui, theme, morph),
            poly_widgets::CurveRegion::Footer => cells(ui, theme, state, edits, &ids),
            poly_widgets::CurveRegion::Header => {
                if is_a {
                    preset_stepper(ui, theme, state, edits);
                }
            }
        },
    );
}

/// The hero: all seven tables stacked, the morph position marking where
/// the scan is. The shapes ARE the signal — the map reads like the sound.
fn wavetable_map(ui: &mut egui::Ui, theme: &Theme, morph: f32) {
    let rect = ui.available_rect_before_wrap();
    let (rect, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
    if rect.width() < 16.0 || rect.height() < 16.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let pad = theme.sp(space::XS);
    let band = (rect.height() - pad * 2.0) / WAVES as f32;
    for wave in 0..WAVES {
        let top = rect.top() + pad + band * wave as f32;
        let w = rect.width() - pad * 2.0;
        let steps = (w as usize).clamp(16, 256);
        let points: Vec<egui::Pos2> = (0..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                let v = loom_wave(wave, t);
                egui::pos2(
                    rect.left() + pad + w * t,
                    top + band * 0.5 - band * 0.36 * v.clamp(-1.0, 1.0),
                )
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(
                1.2,
                if wave == 0 {
                    theme.role_shape
                } else {
                    theme.divider
                },
            ),
        ));
    }
    // The scan marker: where the morph knob is in the map.
    let x = rect.left() + pad + (rect.width() - pad * 2.0) * morph.clamp(0.0, 1.0);
    painter.line_segment(
        [
            egui::pos2(x, rect.top() + pad),
            egui::pos2(x, rect.bottom() - pad),
        ],
        egui::Stroke::new(stroke::BOLD, theme.role_shape.gamma_multiply(0.8)),
    );
    crate::ui::hud::brackets(&painter, rect, egui::Stroke::new(1.0, theme.outline));
}

/// Three rows of two cells, no plot — the knob pages.
fn noise_filter_page(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut LoomUi,
    edits: &mut Vec<ParamEdit>,
) {
    knobs_rows(
        ui,
        theme,
        state,
        edits,
        [
            [lp::F_MODE, lp::F_CUTOFF],
            [lp::F_RES, lp::F_ENV],
            [lp::N_LEVEL, lp::N_DECAY],
            [lp::GAIN, lp::VEL],
        ],
    );
}

fn amp_page(ui: &mut egui::Ui, theme: &Theme, state: &mut LoomUi, edits: &mut Vec<ParamEdit>) {
    knobs_rows(
        ui,
        theme,
        state,
        edits,
        [
            [lp::AMP_A, lp::AMP_D],
            [lp::AMP_S, lp::AMP_R],
            [lp::FENV_A, lp::FENV_D],
            [lp::FENV_S, lp::FENV_R],
        ],
    );
}

fn voice_page(ui: &mut egui::Ui, theme: &Theme, state: &mut LoomUi, edits: &mut Vec<ParamEdit>) {
    knobs_rows(
        ui,
        theme,
        state,
        edits,
        [
            [lp::V_UNISON, lp::V_DETUNE],
            [lp::V_SPREAD, lp::V_GLIDE],
            [lp::LFO_RATE, lp::LFO_PITCH],
            [lp::PENV, lp::PENV_D],
        ],
    );
}

fn knobs_rows(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut LoomUi,
    edits: &mut Vec<ParamEdit>,
    rows: [[u32; 2]; 4],
) {
    let gap = theme.sp(space::XXS);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    let rows_n = rows.len() as f32;
    let height = ((ui.available_height() - gap * (rows_n - 1.0)) / rows_n).max(1.0);
    for row in rows {
        ui.horizontal(|ui| {
            for (drawn, id) in row.iter().enumerate() {
                let left = (row.len() - drawn) as f32;
                let room = ui.available_width() - gap * (left - 1.0).max(0.0);
                let width = (room / left).floor().max(1.0);
                let p = param_of(*id);
                let Some(slot) = state.slot(*id) else {
                    continue;
                };
                let before = *slot;
                ui.allocate_ui_with_layout(
                    egui::vec2(width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        if p.choices().is_some() {
                            poly_widgets::labeled_cell_steps(ui, theme, &p, slot, None);
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &p, slot, None);
                        }
                    },
                );
                if *slot != before {
                    edits.push(ParamEdit {
                        param: *id,
                        value: loom_value(*id, *slot),
                    });
                }
            }
        });
    }
}

fn cells(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut LoomUi,
    edits: &mut Vec<ParamEdit>,
    ids: &[u32; 4],
) {
    let gap = theme.sp(space::XXS);
    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
    let height = ui.available_height().max(1.0);
    ui.horizontal(|ui| {
        for (drawn, id) in ids.iter().enumerate() {
            let left = (ids.len() - drawn) as f32;
            let room = ui.available_width() - gap * (left - 1.0).max(0.0);
            let width = (room / left).floor().max(1.0);
            let p = param_of(*id);
            let Some(slot) = state.slot(*id) else {
                continue;
            };
            let before = *slot;
            ui.allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    if p.choices().is_some() {
                        poly_widgets::labeled_cell_steps(ui, theme, &p, slot, None);
                    } else {
                        poly_widgets::labeled_cell_bar(ui, theme, &p, slot, None);
                    }
                },
            );
            if *slot != before {
                edits.push(ParamEdit {
                    param: *id,
                    value: loom_value(*id, *slot),
                });
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ui::device::probe;

    const MAX_W: f32 = 520.0;
    const MIN_W: f32 = control::POLY_FACE_W;

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

    fn view() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(520.0, 260.0))
    }

    fn pointer_draw(state: &mut LoomUi, path: &[probe::Step]) -> Vec<Vec<ParamEdit>> {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        probe::run(&ctx, view(), path, |ui| loom_card(ui, &theme, state))
    }

    fn flat(frames: &[Vec<ParamEdit>]) -> Vec<ParamEdit> {
        frames.iter().flatten().cloned().collect()
    }

    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let edits = loom_edits(&LoomUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        let mut expect: Vec<u32> = lp::TABLE.iter().map(|p| p.id).collect();
        expect.sort_unstable();
        assert_eq!(ids, expect, "the card does not cover the table");
    }

    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in lp::TABLE {
            for i in 0..=40 {
                let value = loom_value(def.id, i as f32 / 40.0);
                assert!(
                    value.is_finite() && (def.min..=def.max).contains(&value),
                    "{}: position {i}/40 gives {value}",
                    def.name
                );
            }
        }
    }

    #[test]
    fn value_and_norm_round_trip() {
        for def in lp::TABLE {
            for i in 0..=40 {
                let value = loom_value(def.id, i as f32 / 40.0);
                let again = loom_value(def.id, loom_norm(def.id, value));
                let tol = if loom_is_discrete(def.id) {
                    (def.max - def.min) / 64.0 + 1e-3
                } else {
                    value.abs() * 1e-3 + 1e-3
                };
                assert!(
                    (again - value).abs() <= tol,
                    "{}: {value} came back as {again}",
                    def.name
                );
            }
        }
    }

    #[test]
    fn defaults_come_from_the_table() {
        for def in lp::TABLE {
            let mut state = LoomUi::default();
            let norm = *state.slot(def.id).unwrap();
            let value = loom_value(def.id, norm);
            assert!(
                (value - def.default).abs() < (def.max - def.min) * 1e-3,
                "{} opened at {value}, table says {}",
                def.name,
                def.default
            );
        }
    }

    #[test]
    fn fractional_engine_values_use_the_whole_percent_control() {
        for id in [lp::A_MORPH, lp::B_MORPH, lp::AMP_S, lp::FENV_S] {
            assert_eq!(loom_norm(id, 0.0), 0.0, "{} minimum", id_name(id));
            assert_eq!(loom_norm(id, 1.0), 1.0, "{} maximum", id_name(id));
            assert_eq!(loom_value(id, 0.5), 0.5, "{} midpoint", id_name(id));
            assert_eq!(param_of(id).format(0.8), "80 %", "{} readout", id_name(id));
        }
    }

    #[test]
    fn every_row_fits_the_width_the_card_asks_for() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        frame(&ctx, |ui| {
            let declared = face_width(ui, &theme);
            for row in FACE_ROWS {
                let natural = row
                    .iter()
                    .map(|id| cell_min_width(ui, &theme, &param_of(*id)))
                    .sum::<f32>()
                    + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32;
                assert!(
                    natural <= declared + 0.5,
                    "a row needs {natural} pt but Loom asks for {declared} pt"
                );
            }
        });
    }

    #[test]
    fn oscillator_footer_reserves_two_lines_and_keeps_the_hero() {
        assert_eq!(CELL_UNITS, 2, "a value and its name are two lines");
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let unit = theme.sp(control::POLY_CELL_H);
        let footer = unit * CELL_UNITS as f32 + gap * (CELL_UNITS - 1) as f32;
        let preset = footer;
        let tabs = theme.sp(control::POLY_TAB_H);
        let hero = theme.sp(CARD_H) - footer - preset - tabs - gap * 3.0;
        assert!(
            hero > unit * 3.0,
            "only {hero} pt left for the wavetable map"
        );
    }

    #[test]
    fn page_and_preset_view_state_round_trip() {
        for page in 0..TAB_LABELS.len() {
            for preset in 0..PRESET_NAMES.len() {
                let mut before = LoomUi {
                    page,
                    preset_index: preset_param().at_index(preset),
                    ..LoomUi::default()
                };
                let packed = before.packed_view();
                before.page = 0;
                before.preset_index = 0.0;
                before.restore_view(packed);
                assert_eq!(before.page, page);
                assert_eq!(preset_param().index(before.preset_index), preset);
            }
        }
    }

    #[test]
    fn loading_a_preset_keeps_its_stepper_position() {
        let mut state = LoomUi::default();
        for preset in 0..PRESET_NAMES.len() {
            load_preset(&mut state, preset);
            assert_eq!(preset_param().index(state.preset_index), preset);
        }
    }

    #[test]
    fn every_tab_answers_a_real_pointer() {
        let theme = Theme::dark();
        let ctx = egui::Context::default();
        let width = frame(&ctx, |ui| face_width(ui, &theme));
        let y = card::title_height(&theme) + theme.sp(control::POLY_TAB_H) * 0.5;
        for (want, label) in TAB_LABELS.iter().enumerate() {
            let mut state = LoomUi {
                page: (want + 1) % TAB_LABELS.len(),
                ..LoomUi::default()
            };
            let x = width * (want as f32 + 0.5) / TAB_LABELS.len() as f32;
            let frames = pointer_draw(&mut state, &probe::click_path(egui::pos2(x, y)));
            assert_eq!(state.page, want, "{} did not take the click", label);
            assert!(flat(&frames).is_empty(), "a tab click changed a parameter");
        }
    }

    #[test]
    fn oscillator_cells_answer_and_keep_their_own_drag() {
        let theme = Theme::dark();
        let ctx = egui::Context::default();
        let width = frame(&ctx, |ui| face_width(ui, &theme));
        let from = egui::pos2(
            width / 8.0,
            theme.sp(CARD_H) - theme.sp(control::POLY_CELL_H),
        );
        let mut state = LoomUi::default();
        let before = state.a_morph;
        let frames = pointer_draw(
            &mut state,
            &probe::drag_path(from, from + egui::vec2(48.0, -8.0), 8),
        );
        let edits = flat(&frames);
        assert!(state.a_morph > before, "A MORPH did not follow its drag");
        assert!(edits.iter().any(|edit| edit.param == lp::A_MORPH));
        assert!(
            edits.iter().all(|edit| edit.param == lp::A_MORPH),
            "the A MORPH drag leaked to a neighbour: {edits:?}"
        );
    }

    #[test]
    fn the_card_stays_inside_its_budget() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = LoomUi::default();
        for page in 0..TAB_LABELS.len() {
            state.page = page;
            let rect = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| {
                        ui.horizontal(|ui| loom_card(ui, &theme, &mut state))
                            .response
                            .rect
                    })
                    .inner
            });
            let (w, h) = (rect.width(), rect.height());
            assert!(w <= MAX_W, "page {page}: {w:.0} pt wide, over {MAX_W:.0}");
            assert!(
                w >= MIN_W,
                "page {page}: only {w:.0} pt wide — did it draw?"
            );
            let budget = theme.sp(CARD_H) + crate::ui::tokens::stroke::HAIR * 2.0;
            assert!(h <= budget, "page {page}: {h:.0} pt tall, over {budget:.0}");
        }
    }

    #[test]
    fn drawing_at_rest_emits_nothing() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let mut state = LoomUi::default();
        let before = state;
        for _ in 0..2 {
            let edits = frame(&ctx, |ui| {
                egui::CentralPanel::default()
                    .show(ui, |ui| loom_card(ui, &theme, &mut state))
                    .inner
            });
            assert!(edits.is_empty(), "the card emitted {edits:?} at rest");
        }
        assert_eq!(state, before, "drawing the card moved a control");
    }

    /// The map's seven waves are distinct, bounded and periodic — a
    /// wavetable whose pictures can't be told apart is not a wavetable.
    #[test]
    fn the_map_draws_seven_distinct_bounded_waves() {
        let sample = |shape: usize| -> Vec<f32> {
            (0..64).map(|i| loom_wave(shape, i as f32 / 64.0)).collect()
        };
        for shape in 0..WAVES {
            let w = sample(shape);
            assert!(
                w.iter()
                    .all(|v| v.is_finite() && (-1.001..=1.001).contains(v)),
                "wave {shape} left the rails"
            );
            let span = w.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(span > 0.2, "wave {shape} is flat");
        }
        for a in 0..WAVES {
            for b in (a + 1)..WAVES {
                let (x, y) = (sample(a), sample(b));
                let diff: f32 = x.iter().zip(y.iter()).map(|(p, q)| (p - q).abs()).sum();
                assert!(diff > 1.0, "waves {a} and {b} draw the same picture");
            }
        }
    }

    /// The workhorse promise: every preset loads to legal engine values,
    /// and loading emits the WHOLE table so the engine follows the sound.
    #[test]
    fn every_preset_loads_as_the_whole_table() {
        for (index, name) in PRESET_NAMES.iter().enumerate() {
            let mut state = LoomUi::default();
            let edits = load_preset(&mut state, index);
            let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
            ids.sort_unstable();
            let mut expect: Vec<u32> = lp::TABLE.iter().map(|p| p.id).collect();
            expect.sort_unstable();
            assert_eq!(ids, expect, "{name} does not cover the table");
            for edit in &edits {
                let def = lp::TABLE.iter().find(|d| d.id == edit.param).unwrap();
                assert!(
                    edit.value.is_finite() && (def.min..=def.max).contains(&edit.value),
                    "{name}: {} = {} is outside {}..{}",
                    def.name,
                    edit.value,
                    def.min,
                    def.max
                );
            }
        }
    }

    /// Two presets with identical knobs are one preset — the bank's
    /// whole job is to be a map of DIFFERENT sounds.
    #[test]
    fn the_presets_are_distinct() {
        let rows: Vec<Vec<f32>> = (0..PRESET_NAMES.len())
            .map(|index| {
                let mut state = LoomUi::default();
                load_preset(&mut state, index);
                lp::TABLE
                    .iter()
                    .map(|def| state.norm(def.id).unwrap_or(0.0))
                    .collect()
            })
            .collect();
        for a in 0..rows.len() {
            for b in (a + 1)..rows.len() {
                let diff: f32 = rows[a]
                    .iter()
                    .zip(rows[b].iter())
                    .map(|(x, y)| (x - y).abs())
                    .sum();
                assert!(
                    diff > 0.5,
                    "{} and {} are the same patch",
                    PRESET_NAMES[a],
                    PRESET_NAMES[b]
                );
            }
        }
    }
}
