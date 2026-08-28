//! The reverb device card.
//!
//! Nine controls over two rows, laid out by the rules in
//! `notes/20260827-device-card-layout.md`: a labelled cell is two
//! `POLY_CELL_H` units tall, the card declares its own width, and a row
//! shares that width progressively rather than in equal shares.
//!
//! The strip's two rows are the two halves of the device. The first
//! describes the SPACE — where it starts, how big, how long, how dark,
//! and what the tail keeps of the low end. The second describes how that
//! space is PRESENTED — how dense, how much it moves, how wide, how much
//! you hear. Learning the split is most of learning the reverb.
//!
//! Above them the hero draws the room answering one dry impulse: a
//! source tick, the pre-delay's silence, a run of early reflections, and
//! then the tail itself as the crowd of echoes the decay number stands
//! for. See [`tail`].

use crate::params::{self, reverb as rp};
use crate::ui::device::synth::ParamEdit;
use crate::ui::device::{
    Footprint, Mapping, Param, Unit, Well, Wells, card, design, metrics, poly_widgets,
};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// Knob positions of one reverb, normalized. Serialized into project
/// files, so knob positions survive a reload.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ReverbUi {
    pub predelay: f32,
    pub size: f32,
    pub decay: f32,
    pub damp: f32,
    pub low_cut: f32,
    pub diffusion: f32,
    pub modulation: f32,
    pub width: f32,
    pub mix: f32,
}

impl Default for ReverbUi {
    /// Every knob at the TABLE's default, so a fresh card and a fresh
    /// node agree without either asking the other.
    fn default() -> Self {
        Self::from_engine(|id| params::def(rp::TABLE, id).default)
    }
}

impl ReverbUi {
    /// The card's state for a patch in engine units.
    ///
    /// Takes a READER rather than the engine's params struct: a widget
    /// module must not know `crate::audio`, and the app is the layer that
    /// knows both sides.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let at = |id: u32| reverb_norm(id, get(id));
        Self {
            predelay: at(rp::PREDELAY),
            size: at(rp::SIZE),
            decay: at(rp::DECAY),
            damp: at(rp::DAMP),
            low_cut: at(rp::LOWCUT),
            diffusion: at(rp::DIFFUSION),
            modulation: at(rp::MODULATION),
            width: at(rp::WIDTH),
            mix: at(rp::MIX),
        }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        Some(match param {
            rp::PREDELAY => &mut self.predelay,
            rp::SIZE => &mut self.size,
            rp::DECAY => &mut self.decay,
            rp::DAMP => &mut self.damp,
            rp::LOWCUT => &mut self.low_cut,
            rp::DIFFUSION => &mut self.diffusion,
            rp::MODULATION => &mut self.modulation,
            rp::WIDTH => &mut self.width,
            rp::MIX => &mut self.mix,
            _ => return None,
        })
    }

    fn get(&self, param: u32) -> f32 {
        match param {
            rp::PREDELAY => self.predelay,
            rp::SIZE => self.size,
            rp::DECAY => self.decay,
            rp::DAMP => self.damp,
            rp::LOWCUT => self.low_cut,
            rp::DIFFUSION => self.diffusion,
            rp::MODULATION => self.modulation,
            rp::WIDTH => self.width,
            _ => self.mix,
        }
    }
}

/// The strip, in two rows. See the module note for why it is two.
const ROWS: [&[u32]; 2] = [
    &[rp::PREDELAY, rp::SIZE, rp::DECAY, rp::DAMP, rp::LOWCUT],
    &[rp::DIFFUSION, rp::MODULATION, rp::WIDTH, rp::MIX],
];

/// A labelled cell prints its value on one line and its name on the next.
const CELL_UNITS: usize = 2;
const FOOTER_ROWS: usize = ROWS.len() * CELL_UNITS;

/// One control by wire id — the single place an id becomes a `Param`.
///
/// NATURAL UNITS end to end: milliseconds, hertz, seconds, percent. The
/// times and frequencies are LOG, because the difference between 10 ms
/// and 20 ms of pre-delay is the whole character of the effect and the
/// difference between 190 and 200 is nothing.
fn param_of(param: u32) -> Param {
    let def = params::def(rp::TABLE, param);
    let log = |name: &'static str, unit| {
        Param::new(
            name,
            Mapping::Log {
                min: def.min.max(0.001),
                max: def.max,
            },
            unit,
        )
        .with_default(def.default)
    };
    match param {
        // Pre-delay reaches zero, which a log map cannot — so it is
        // linear, and its useful range is short enough that it reads
        // well anyway.
        rp::PREDELAY => Param::new(
            "predelay",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Ms,
        )
        .with_default(def.default),
        rp::SIZE => Param::new(
            "size",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Plain,
        )
        .with_default(def.default),
        rp::DECAY => log("decay", Unit::Seconds),
        rp::DAMP => log("damp", Unit::Hz),
        rp::LOWCUT => log("low cut", Unit::Hz),
        rp::DIFFUSION => Param::percent("diffuse").with_default(def.default * 100.0),
        rp::MODULATION => Param::new(
            "mod",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Plain,
        )
        .with_default(def.default),
        // NOT `Param::percent`: that maps 0..100, and width runs to 200 %
        // so that 100 % can be the network's own spread rather than an
        // arbitrary middle. A percent param here silently clamps 150 %
        // back to 100 and the knob stops halfway.
        rp::WIDTH => Param::new(
            "width",
            Mapping::Linear {
                min: def.min * 100.0,
                max: def.max * 100.0,
            },
            Unit::Percent,
        )
        .with_default(def.default * 100.0),
        _ => Param::percent("mix").with_default(def.default * 100.0),
    }
}

/// What the widget SHOWS for an engine value.
fn shown(param: u32, value: f32) -> f32 {
    match param {
        rp::DIFFUSION | rp::MIX => value * 100.0,
        // Width runs 0..2 in the engine and 0..200 % on the dial, so
        // "100 %" is the network's own spread rather than an arbitrary
        // middle.
        rp::WIDTH => value * 100.0,
        _ => value,
    }
}

/// What the ENGINE receives, clamped through the table so a knob at
/// either stop cannot emit a letter the engine has to bin.
fn natural(param: u32, value: f32) -> f32 {
    let raw = match param {
        rp::DIFFUSION | rp::MIX | rp::WIDTH => value / 100.0,
        _ => value,
    };
    params::def(rp::TABLE, param).clamp(raw)
}

pub fn reverb_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

pub fn reverb_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// Whether a parameter lives on a LOG scale — so a modulation sweep of a
/// decay moves in doublings exactly where the knob does.
pub fn reverb_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, for a reset, a preset recall, or the
/// moment the device is first loaded.
pub fn reverb_edits(state: &ReverbUi) -> Vec<ParamEdit> {
    rp::TABLE
        .iter()
        .map(|def| ParamEdit {
            param: def.id,
            value: reverb_value(def.id, state.get(def.id)),
        })
        .collect()
}

/// The narrowest a cell may be drawn: room for the widest thing it will
/// ever print, and nothing over.
fn cell_min_width(ui: &egui::Ui, theme: &Theme, param: &Param) -> f32 {
    let value = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &param.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the face needs: its widest row at its narrowest.
fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    ROWS.iter()
        .map(|row| {
            let cells: f32 = row
                .iter()
                .map(|id| cell_min_width(ui, theme, &param_of(*id)))
                .sum();
            cells + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32
        })
        .fold(0.0, f32::max)
}

/// How far the decay plot looks, in seconds.
///
/// FIXED, not scaled to the reverb, for the reason the echo's window is:
/// a picture that stretched to fit the current tail would draw every
/// room identically. A fixed axis means a booth decays to nothing
/// halfway across and a hall runs off the edge — which is what those two
/// settings sound like.
const PLOT_SECONDS: f32 = 6.0;

/// The most tail marks the picture draws.
///
/// A bound, not a taste: the cloud stands in for a crowd of echoes too
/// dense to count, and a panel is about this many marks wide before they
/// stop reading as separate echoes.
const MAX_TAIL_MARKS: usize = 320;

/// A stable 0..1 hash. The picture is rebuilt from engine units every
/// frame, so every crumb of randomness it shows must come from here and
/// only from here — a per-frame roll would make the cloud shimmer under
/// a still patch.
fn hash01(seed: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9).wrapping_add(0x7F4A_7C15);
    x ^= x >> 16;
    x = x.wrapping_mul(0xC2B2_AE3D);
    x ^= x >> 15;
    (x >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
}

/// The tail's level at `seconds` after the source, as a fraction of
/// full: silent while the pre-delay counts, −60 dB one decay time after
/// the room wakes. The drawing and the tests share this so the picture
/// cannot drift from the number it illustrates.
fn tail_level(predelay_s: f32, rt60: f32, seconds: f32) -> f32 {
    if seconds < predelay_s {
        0.0
    } else {
        10f32.powf(-3.0 * (seconds - predelay_s) / rt60.max(0.01))
    }
}

/// The mean gap between tail marks, in seconds. DIFFUSION densifies the
/// cloud — a diffused tail is a wash, an undiffused one is a scatter of
/// separate echoes — and a bigger SIZE keeps the first echoes apart.
fn cloud_gap_mean(size01: f32, diffusion: f32) -> f32 {
    egui::lerp(0.024..=0.007, diffusion.clamp(0.0, 1.0)) + size01.clamp(0.0, 1.0) * 0.008
}

/// How bright the tail still is `elapsed` seconds after the room woke.
///
/// DAMP is the corner of the lowpass in every feedback path, so the
/// highs are absorbed once per trip around the room: the lower the
/// corner, the faster the tail goes dark. `1000 / damp` is the
/// time-constant that reads right at both ends of the knob — a 400 Hz
/// room dims within a second and an 18 kHz room never visibly does.
fn damp_brightness(damp_hz: f32, elapsed: f32) -> f32 {
    (-1000.0 * elapsed / damp_hz.max(1.0)).exp()
}

/// Blend two colors in gamma space. Small, local, and only for strokes.
fn mix_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |lo: u8, hi: u8| (lo as f32 + (hi as f32 - lo as f32) * t).round() as u8;
    egui::Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// The tail, drawn as the impulse response it actually is.
///
/// A reverb's hero is its DECAY, but a bare envelope reads as an
/// equation, not a room. So the picture is the room answering one dry
/// impulse: the source tick at the left, the pre-delay's silence, a run
/// of early reflections while the sound finds its way around — spaced by
/// SIZE, multiplied by DIFFUSION — and then the tail itself as the crowd
/// of echoes the envelope stands for. WIDTH walks the two channels apart
/// so the crowd splits into a haze, DAMP dims it as the highs are
/// absorbed, and the envelope rides underneath as a hairline so the
/// decay time stays exactly readable.
fn tail(ui: &mut egui::Ui, theme: &Theme, state: &ReverbUi) {
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let columns = (rect.width().ceil() as usize).clamp(2, 1_024);
    let predelay_s = reverb_value(rp::PREDELAY, state.predelay) * 0.001;
    let rt60 = reverb_value(rp::DECAY, state.decay).max(0.01);
    let size01 = state.size.clamp(0.0, 1.0);
    let diffusion = reverb_value(rp::DIFFUSION, state.diffusion).clamp(0.0, 1.0);
    let damp_hz = reverb_value(rp::DAMP, state.damp).max(1.0);
    let width = reverb_value(rp::WIDTH, state.width).clamp(0.0, 2.0);
    let modulation = reverb_value(rp::MODULATION, state.modulation).clamp(0.0, 8.0);
    let mix = reverb_value(rp::MIX, state.mix);

    // Height is the WET level, so the picture is of what you will hear,
    // not of what the room is doing privately. The 5 % floor keeps a
    // hint of the shape at mix zero, the way the echo's picture does.
    let base = rect.bottom();
    let full = rect.height() * 0.9 * mix.max(0.05);
    let x_at = |seconds: f32| rect.left() + (seconds / PLOT_SECONDS).clamp(0.0, 1.0) * rect.width();

    // The room lives in a fixed six-second observation window. One-second
    // registrations make short rooms visibly short and long rooms visibly run
    // off the instrument instead of auto-fitting every patch to the same tail.
    for second in 0..=PLOT_SECONDS as usize {
        let x = x_at(second as f32);
        painter.vline(
            x,
            rect.y_range(),
            egui::Stroke::new(
                stroke::HAIR,
                if second % 2 == 0 {
                    theme.grid_beat
                } else {
                    theme.grid_sub
                },
            ),
        );
        painter.text(
            egui::pos2(x, base - theme.sp(space::XXS)),
            if second == 0 {
                egui::Align2::LEFT_BOTTOM
            } else if second as f32 == PLOT_SECONDS {
                egui::Align2::RIGHT_BOTTOM
            } else {
                egui::Align2::CENTER_BOTTOM
            },
            format!("{second}"),
            egui::FontId::monospace(font::MICRO_LABEL),
            theme.text_muted,
        );
    }
    painter.hline(
        rect.x_range(),
        base - stroke::HAIR,
        egui::Stroke::new(stroke::BOLD, theme.grid_beat),
    );

    // PRE is where the room wakes; RT60 is where its envelope reaches
    // −60 dB. Both are direct coordinates of the controls, exposed as terse
    // hardware registrations rather than another envelope legend.
    let pre_x = x_at(predelay_s);
    painter.line_segment(
        [
            egui::pos2(pre_x, rect.top()),
            egui::pos2(pre_x, rect.top() + theme.sp(space::SM)),
        ],
        egui::Stroke::new(stroke::BOLD, theme.role_mod),
    );
    let rt60_at = predelay_s + rt60;
    let rt60_x = x_at(rt60_at);
    painter.line_segment(
        [
            egui::pos2(rt60_x, base - theme.sp(space::SM)),
            egui::pos2(rt60_x, base),
        ],
        egui::Stroke::new(stroke::BOLD, theme.role_time),
    );
    if rt60_at > PLOT_SECONDS {
        painter.line_segment(
            [
                egui::pos2(rt60_x - theme.sp(space::XS), base - theme.sp(space::XS)),
                egui::pos2(rt60_x, base - theme.sp(space::SM)),
            ],
            egui::Stroke::new(stroke::BOLD, theme.role_time),
        );
    }

    // The envelope, as a hairline: the −60 dB promise the decay control
    // makes, drawn dim because it is the ground everything else sits on.
    let mut guide = Vec::with_capacity(columns);
    for column in 0..columns {
        let along = column as f32 / (columns - 1).max(1) as f32;
        let seconds = along * PLOT_SECONDS;
        guide.push(egui::pos2(
            x_at(seconds),
            base - tail_level(predelay_s, rt60, seconds) * full,
        ));
    }
    painter.add(egui::Shape::line(
        guide,
        egui::Stroke::new(stroke::HAIR, theme.role_time_dim),
    ));

    // The tail cloud: a crowd of echoes riding the envelope, denser the
    // more diffused the network is. The two channels walk apart as WIDTH
    // grows — at zero they land on the same mark, which is what mono
    // means — and MODULATION scatters the spacing a few samples' worth.
    let gap = cloud_gap_mean(size01, diffusion);
    let wander = modulation / 8.0 * 0.004;
    let mut t = predelay_s + gap * hash01(0x7A11) * 0.5;
    for i in 0..MAX_TAIL_MARKS {
        if t > PLOT_SECONDS {
            break;
        }
        let h = tail_level(predelay_s, rt60, t) * full;
        if h < 1.0 {
            // The level only falls from here on; nothing later is taller.
            break;
        }
        // DAMP as brightness: the tail keeps its highs at the top of the
        // knob and goes dark early at the bottom.
        let color = mix_color(
            theme.role_time_dim,
            theme.role_time,
            damp_brightness(damp_hz, t - predelay_s),
        );
        let decorrelate = width * 0.006;
        for (channel, salt) in [(0u32, 0x101), (1u32, 0x102)] {
            let off = (hash01(i as u32 + salt) - 0.5) * 2.0 * decorrelate * channel as f32;
            let x = x_at(t + off);
            painter.line_segment(
                [egui::pos2(x, base), egui::pos2(x, base - h)],
                egui::Stroke::new(stroke::HAIR, color),
            );
        }
        t += gap * hash01(i as u32 + 0x5EED) * 2.0 + wander * (hash01(i as u32 + 0x3A) - 0.5) * 2.0;
    }

    // Early reflections: the room's first answer, while the sound is
    // still finding its way around. SIZE spaces them — a hall answers
    // sparser and later than a booth — and DIFFUSION decides how many
    // there are before the smear takes over.
    let er_count = 3 + (diffusion * 12.0).round() as usize;
    let mut er_t = predelay_s + egui::lerp(0.004..=0.030, size01);
    for i in 0..er_count {
        if er_t > PLOT_SECONDS {
            break;
        }
        let h = 0.8 * 0.68f32.powi(i as i32) * full;
        if h < 1.0 {
            // Later reflections are only quieter.
            break;
        }
        let x = x_at(er_t);
        painter.line_segment(
            [egui::pos2(x, base), egui::pos2(x, base - h)],
            egui::Stroke::new(stroke::MARK, theme.role_time),
        );
        // Gaps grow as the cloud takes over: reflections run out of
        // fresh wall to answer from.
        let gap = egui::lerp(0.006..=0.030, size01) * (1.0 + 0.4 * i as f32);
        er_t += gap * (0.6 + 0.8 * hash01(i as u32 + 0xE0));
    }

    // The source tick: what the room is answering, drawn dim so the
    // answer stays the loudest thing on the card.
    painter.line_segment(
        [
            egui::pos2(rect.left(), base - rect.height() * 0.45),
            egui::pos2(rect.left(), base),
        ],
        egui::Stroke::new(stroke::MARK, theme.role_level_dim),
    );

    // The corner tag, Elektron-style: what the axis measures and how
    // loud the answer is, for when your eyes are on the picture rather
    // than the cells.
    painter.text(
        rect.left_top() + egui::vec2(design::gap(theme), design::gap(theme)),
        egui::Align2::LEFT_TOP,
        format!(
            "{}  {}",
            param_of(rp::DECAY).format(state.decay),
            param_of(rp::MIX).format(state.mix)
        ),
        egui::FontId::monospace(font::LABEL),
        theme.text_muted,
    );
    painter.text(
        egui::pos2(rect.center().x, rect.top() + design::gap(theme)),
        egui::Align2::CENTER_TOP,
        "IMPULSE FIELD // 6.0 S",
        egui::FontId::proportional(font::MICRO_LABEL),
        theme.role_time_dim,
    );
    painter.text(
        egui::pos2(
            rect.right() - design::gap(theme),
            rect.top() + design::gap(theme),
        ),
        egui::Align2::RIGHT_TOP,
        format!("L/R {:.0}%", width * 100.0),
        egui::FontId::monospace(font::MICRO_LABEL),
        theme.role_time_dim,
    );
}

/// Draw the reverb card. Returns the edits the user just made.
pub fn reverb_card(ui: &mut egui::Ui, theme: &Theme, state: &mut ReverbUi) -> Vec<ParamEdit> {
    let mut edits = Vec::new();
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::card_sized(ui, theme, "reverb", control::DEVICE_TALL_H, |ui| {
        card::wells(ui, theme, &layout, |ui, _| {
            poly_widgets::dark_curve_panel(ui, theme, None, 0.0, 0, FOOTER_ROWS, |ui, region| {
                match region {
                    poly_widgets::CurveRegion::Plot => tail(ui, theme, state),
                    poly_widgets::CurveRegion::Footer => footer(ui, theme, state, &mut edits),
                    poly_widgets::CurveRegion::Header => {}
                }
            });
        });
    });
    edits
}

fn footer(ui: &mut egui::Ui, theme: &Theme, state: &mut ReverbUi, edits: &mut Vec<ParamEdit>) {
    let gap = theme.sp(space::XXS);
    let rows = ROWS.len() as f32;
    // The gaps come off FIRST — see the layout note.
    let height = ((ui.available_height() - gap * (rows - 1.0)) / rows).max(1.0);
    ui.spacing_mut().item_spacing.y = gap;
    for row in ROWS {
        ui.horizontal(|ui| {
            let gap_x = ui.spacing().item_spacing.x;
            for (drawn, param) in row.iter().enumerate() {
                let spec = param_of(*param);
                // Progressive share, recomputed from what is left.
                let left = (row.len() - drawn) as f32;
                let room = ui.available_width() - gap_x * (left - 1.0).max(0.0);
                let width = (room / left).floor().max(1.0);
                let Some(norm) = state.slot(*param) else {
                    continue;
                };
                ui.allocate_ui_with_layout(
                    egui::vec2(width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        ui.set_height(height);
                        if poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None) {
                            edits.push(ParamEdit {
                                param: *param,
                                value: reverb_value(*param, *norm),
                            });
                        }
                    },
                );
            }
        });
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn every_table_knob_leaves_as_an_edit() {
        let edits = reverb_edits(&ReverbUi::default());
        assert_eq!(edits.len(), rp::TABLE.len());
        for def in rp::TABLE {
            let edit = edits
                .iter()
                .find(|edit| edit.param == def.id)
                .unwrap_or_else(|| panic!("{} never left the card", def.name));
            assert!(
                (edit.value - def.default).abs() <= (def.default.abs() * 1e-3).max(1e-3),
                "{} defaults to {} not {}",
                def.name,
                edit.value,
                def.default
            );
        }
    }

    #[test]
    fn every_parameter_round_trips_through_the_card() {
        for def in rp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = reverb_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left its range: {value}",
                    def.name
                );
                let again = reverb_value(def.id, reverb_norm(def.id, value));
                assert!(
                    (again - value).abs() <= (value.abs() * 1e-3).max(1e-3),
                    "{} did not round-trip: {value} -> {again}",
                    def.name
                );
            }
        }
    }

    /// The layout note's two rules, checked rather than remembered.
    #[test]
    fn the_strip_fits_the_width_and_height_it_asks_for() {
        assert_eq!(CELL_UNITS, 2, "a value and its name are two lines");
        assert_eq!(FOOTER_ROWS, ROWS.len() * CELL_UNITS);
        let theme = Theme::dark();
        let gap = theme.sp(space::XXS);
        let unit = theme.sp(control::POLY_CELL_H);
        let reserved = unit * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS - 1) as f32;
        let needed = unit * CELL_UNITS as f32 * ROWS.len() as f32 + gap * (ROWS.len() - 1) as f32;
        assert!(reserved >= needed, "{reserved} reserved for {needed}");
        assert!(
            control::DEVICE_TALL_H - reserved > unit * 3.0,
            "the tail has no room left"
        );

        let context = egui::Context::default();
        let mut run = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                let declared = face_width(ui, &theme);
                for row in ROWS {
                    let natural: f32 = row
                        .iter()
                        .map(|id| cell_min_width(ui, &theme, &param_of(*id)))
                        .sum::<f32>()
                        + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32;
                    assert!(
                        natural <= declared + 0.5,
                        "{natural} needed, {declared} asked"
                    );
                }
            },
        );
        run.textures_delta.clear();
    }

    #[test]
    fn the_rows_cover_every_parameter_exactly_once() {
        let mut seen: Vec<u32> = ROWS.iter().flat_map(|row| row.iter().copied()).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on the card twice");
        assert_eq!(seen.len(), rp::TABLE.len(), "a parameter is missing");
    }

    /// THE PLOT IS THE DECAY. A tail drawn from anything but the decay
    /// time and the pre-delay would be a picture of a different reverb.
    ///
    /// `tail_level` is the formula the drawing uses, so it is pinned to
    /// an INDEPENDENT restatement of the physics — otherwise a typo in
    /// one place could ship as the picture of a wrong room.
    #[test]
    fn the_tail_plot_follows_predelay_and_decay() {
        let state = ReverbUi {
            predelay: reverb_norm(rp::PREDELAY, 100.0),
            decay: reverb_norm(rp::DECAY, 2.0),
            mix: reverb_norm(rp::MIX, 1.0),
            ..ReverbUi::default()
        };
        let predelay_s = reverb_value(rp::PREDELAY, state.predelay) * 0.001;
        assert!((predelay_s - 0.1).abs() < 0.005, "{predelay_s}");
        let rt60 = reverb_value(rp::DECAY, state.decay);
        assert!((rt60 - 2.0).abs() < 0.02, "{rt60}");

        // Silent before the room answers, and 60 dB down one decay time
        // after it starts.
        let level = |seconds: f32| {
            if seconds < predelay_s {
                0.0
            } else {
                10f32.powf(-3.0 * (seconds - predelay_s) / rt60)
            }
        };
        assert_eq!(level(0.05), 0.0, "silent during the pre-delay");
        assert!(
            (level(predelay_s) - 1.0).abs() < 1e-4,
            "full when it starts"
        );
        let after = level(predelay_s + rt60);
        assert!(
            (after - 0.001).abs() < 1e-4,
            "one decay time should be -60 dB, got {after}"
        );
        for at in [0.0, 0.05, predelay_s, predelay_s + rt60, 3.0, 5.9] {
            assert!(
                (tail_level(predelay_s, rt60, at) - level(at)).abs() < 1e-5,
                "tail_level drifted from the reference at {at}"
            );
        }
    }

    /// The cloud is denser when the network is diffused, and a bigger
    /// room keeps its echoes further apart — the two numbers the picture
    /// draws from, checked as the monotonic relations they are.
    #[test]
    fn diffusion_densifies_and_size_spaces() {
        assert!(
            cloud_gap_mean(0.5, 1.0) < cloud_gap_mean(0.5, 0.0),
            "a diffused tail should be a denser cloud"
        );
        assert!(
            cloud_gap_mean(1.0, 0.5) > cloud_gap_mean(0.0, 0.5),
            "a bigger room should keep its echoes apart"
        );
    }

    /// DAMP is drawn as a brightness, not a length: at the same instant a
    /// darker room is dimmer, the cloud is full-strength the moment the
    /// room wakes, and it never quite winks out.
    #[test]
    fn damp_darkens_without_shortening() {
        let dark = damp_brightness(400.0, 0.5);
        let bright = damp_brightness(18_000.0, 0.5);
        assert!(dark < bright, "a 400 Hz damp should dim before 18 kHz");
        assert!(
            (damp_brightness(5_000.0, 0.0) - 1.0).abs() < 1e-6,
            "full brightness at the first reflection"
        );
        assert!(
            damp_brightness(400.0, 6.0) > 0.0,
            "the low end still rings, however dark"
        );
    }

    /// A card rebuilt from the same state must draw the same picture:
    /// the cloud is hashed, and a hero that shimmers under a still patch
    /// reads as a broken one.
    #[test]
    fn the_picture_does_not_shimmer_at_rest() {
        let theme = Theme::dark();
        let mut state = ReverbUi {
            predelay: reverb_norm(rp::PREDELAY, 80.0),
            decay: reverb_norm(rp::DECAY, 3.5),
            width: reverb_norm(rp::WIDTH, 1.6),
            diffusion: reverb_norm(rp::DIFFUSION, 0.6),
            ..ReverbUi::default()
        };
        let mut render = || {
            let context = egui::Context::default();
            let mut run = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 400.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    reverb_card(ui, &theme, &mut state);
                },
            );
            run.textures_delta.clear();
            run.shapes
        };
        assert_eq!(render(), render(), "the hero shimmers between frames");
    }

    #[test]
    fn drawing_at_rest_emits_nothing() {
        let context = egui::Context::default();
        let theme = Theme::dark();
        let mut state = ReverbUi::default();
        let before = state;
        let mut edits = Vec::new();
        let mut run = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                edits = reverb_card(ui, &theme, &mut state);
            },
        );
        run.textures_delta.clear();
        assert!(edits.is_empty(), "the card moved on its own: {edits:?}");
        assert_eq!(state, before);
    }
}
