//! The sampler's card.
//!
//! Its hero is the WAVEFORM, and the waveform is on every page. The five
//! tabs change which knobs are in reach; they never take the picture
//! away. That is the whole ease argument for a device with thirty-six
//! parameters: you are always looking at the thing you are editing, and
//! the pages are about where your hands are rather than about what mode
//! the machine is in.
//!
//! Design and reasoning: `notes/20260827-sampler-brief.md`.
//!
//! # What lives here and what does not
//!
//! This module owns the PARAMETER half — the one place a wire id becomes
//! a `Param`, the normalized state a project stores, and the value strip.
//! It also draws the waveform, the region, the loop and the slice
//! markers, read-only.
//!
//! The plot is directly editable: start, end, loop and slice handles each
//! own their interaction, the body pans and zooms about the pointer, and
//! the same surface expands full size without changing gesture grammar.
//! Every edit leaves through [`SamplerOutcome`] so the card never owns a
//! second, drifting copy of the instrument.

use super::{
    Footprint, Mapping, Param, ParamEdit, Unit, Well, Wells, card, metrics, poly_widgets, switch,
};
use crate::params::{self, sampler as sp};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::theme::Theme;
use crate::ui::tokens::{control, font, space, stroke};
use eframe::egui;

/// The card's stored knob positions, all normalized `0..=1`.
///
/// Normalized rather than in engine units for the reason every other card
/// here is: the widget speaks positions, the engine speaks hertz and
/// milliseconds, and one conversion in one place is what keeps them from
/// disagreeing.
/// NOT serialized: every value here is derived from the device's engine
/// units on the way in and handed straight back on the way out, so a
/// second stored copy could only ever disagree with the first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplerUi {
    /// One slot per table row, indexed by wire id. An array rather than
    /// thirty-six named fields: the names would be a second copy of the
    /// table, and a second copy of a table is a table that drifts.
    ///
    /// The array is exactly `TABLE.len()` long and a test says so.
    pub knobs: [f32; ROWS_TOTAL],
}

/// How many rows the table has. Checked against `sp::TABLE` by a test —
/// a constant here and a table there is precisely the drift this layout
/// is trying to avoid, so it is asserted rather than trusted.
pub const ROWS_TOTAL: usize = sp::TABLE.len();

impl Default for SamplerUi {
    /// Every knob at the TABLE's default, so a fresh card and a fresh
    /// voice agree without either asking the other.
    fn default() -> Self {
        Self::from_engine(|id| params::def(sp::TABLE, id).default)
    }
}

impl SamplerUi {
    /// The card's state for a patch in engine units.
    ///
    /// Takes a READER rather than the engine's params struct, because a
    /// widget module must not know `crate::audio` — that is the UI layer
    /// contract. The app knows both sides and hands the values across.
    pub fn from_engine(get: impl Fn(u32) -> f32) -> Self {
        let mut knobs = [0.0f32; ROWS_TOTAL];
        for def in sp::TABLE {
            if let Some(slot) = knobs.get_mut(def.id as usize) {
                *slot = sampler_norm(def.id, get(def.id));
            }
        }
        Self { knobs }
    }

    fn slot(&mut self, param: u32) -> Option<&mut f32> {
        self.knobs.get_mut(param as usize)
    }

    pub fn get(&self, param: u32) -> f32 {
        self.knobs.get(param as usize).copied().unwrap_or(0.0)
    }

    /// Put a control at a normalized position by wire id — what the app
    /// uses to reflect a loaded patch onto the card.
    pub fn set_norm(&mut self, param: u32, norm: f32) {
        if let Some(slot) = self.slot(param) {
            *slot = norm.clamp(0.0, 1.0);
        }
    }
}

/// What the app knows and the card cannot: the file that is loaded.
///
/// Borrowed rather than owned, and rebuilt every frame, because a card
/// owns no state the instance cannot hand back.
#[derive(Default)]
pub struct SamplerView<'a> {
    /// File name to print, or `""` when nothing is loaded.
    pub name: &'a str,
    /// The waveform, already reduced to columns by the app.
    ///
    /// Columns rather than a `Peaks` handle, and deliberately: the peak
    /// pyramid lives in the BINARY crate, and a widget module that
    /// reached into it would be a widget module that knows about files.
    /// The app owns the reduction; this owns the drawing.
    pub wave: &'a [WaveColumn],
    pub frames: u64,
    /// Slice boundaries in source frames, sorted, first is always 0.
    pub slices: &'a [u64],
    /// Read positions of the sounding voices, as `0..=1` of the file.
    pub voices: &'a [f32],
    /// The five-minute cap cut the file short.
    pub truncated: bool,
    /// The file's rate before load-time resampling, 0 if unknown.
    pub original_rate: u32,
}

/// One column of the drawn waveform: the extremes over its span, and the
/// RMS across the same frames.
///
/// The RMS is carried rather than derived because it cannot be: two
/// columns with the same extremes can hold wildly different energy, and
/// that difference is exactly what makes a quiet passage inside a loud
/// file legible at this size.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WaveColumn {
    pub min: f32,
    pub max: f32,
    pub rms: f32,
}

/// Everything the card has to say for a frame.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SamplerOutcome {
    pub edits: Vec<ParamEdit>,
    /// Which tab is open now. The caller stores it on the instance,
    /// because a card is rebuilt every frame and anything it alone
    /// remembered would be forgotten before the next one drew.
    pub page: u8,
    /// A slice marker was dragged: `(index, new source frame)`.
    ///
    /// NOT a `ParamEdit`: a slice table is not a parameter, it is
    /// compiled data, and routing it through the letter path would mean
    /// inventing thirty-six more wire ids for something that is already
    /// a list.
    pub slice_moved: Option<(usize, u64)>,
    /// The user asked to rebuild the slice table from the current
    /// `slices` / `slicefrom` settings.
    pub reslice: bool,
    /// Where the display is looking: how far in, and the left edge as a
    /// fraction of the whole file.
    ///
    /// Handed back rather than kept, because a card is rebuilt every
    /// frame — see `notes/20260826-device-ui-contract.md`, rule 3. The
    /// app stores it on the instance beside `page`.
    pub zoom: f32,
    pub scroll: f32,
    /// The user asked for the display FULL SIZE.
    ///
    /// A request, not a state: which sampler is expanded is the app's,
    /// because a card is rebuilt every frame and an overlay that outlives
    /// the widget that asked for it cannot live inside it.
    pub expand: bool,
    /// ...and, from the expanded view, asked to come back.
    pub collapse: bool,
}

// ---------------------------------------------------------- parameters ---

/// One control by wire id. The single place an id becomes a `Param`, so
/// nothing below can disagree about which knob is which.
///
/// NATURAL UNITS end to end: hertz, milliseconds, semitones, decibels,
/// note names — never `0..1` for something that has a unit. Times and
/// frequencies are LOG, because the difference between 1 ms and 2 ms is
/// the whole character of a declick and the difference between 8000 and
/// 8001 is nothing at all.
/// The bottom of a time knob that can also be OFF, in milliseconds.
///
/// Three of the times here — both edge fades and the loop crossfade —
/// have a table minimum of zero, and zero has no place on a logarithmic
/// scale. So the knob's own scale bottoms out here, at a twentieth of a
/// millisecond, and the very bottom of its travel reports an EXACT zero.
///
/// The rounding is not a fudge: `TIME_FLOOR_MS` at 48 kHz is two and a
/// half samples, which is inaudible as a fade and unreachable as a
/// gesture. What it buys is that "off" stays reachable — and off is
/// load-bearing, because a loop crossfade of zero is the one that repeats
/// bit-exactly and a fade of zero is the one the transparency claim
/// needs.
const TIME_FLOOR_MS: f32 = 0.05;

/// The three rows that get a zero stop.
fn has_zero_stop(param: u32) -> bool {
    matches!(param, sp::FADE_IN | sp::FADE_OUT | sp::LOOP_XFADE)
}

fn param_of(param: u32) -> Param {
    let def = params::def(sp::TABLE, param);
    if param >= sp::PLAYBACK {
        let choices = sp::extra_choices(param);
        if !choices.is_empty() {
            return Param::choice(def.name, choices).with_default(def.default);
        }
        let percent = sp::extra_percent(param);
        let factor = if percent { 100.0 } else { 1.0 };
        let unit = match param {
            sp::WINDOW | sp::GLIDE | sp::MIN_GAP => Unit::Ms,
            sp::COMB_DAMP => Unit::Hz,
            sp::ENV_PITCH | sp::ENV_FILTER => Unit::Semitones,
            _ => Unit::Plain,
        };
        let mapping = if matches!(param, sp::WINDOW | sp::COMB_DAMP | sp::LOOP_SIZE) {
            Mapping::Log {
                min: def.min * factor,
                max: def.max * factor,
            }
        } else {
            Mapping::Linear {
                min: def.min * factor,
                max: def.max * factor,
            }
        };
        return Param::new(def.name, mapping, unit).with_default(def.default * factor);
    }

    let with = |p: Param| p.with_default(shown(param, def.default));
    let log = |name: &'static str, unit| {
        with(Param::new(
            name,
            Mapping::Log {
                min: if has_zero_stop(param) {
                    TIME_FLOOR_MS
                } else {
                    def.min.max(0.001)
                },
                max: def.max,
            },
            unit,
        ))
    };
    // A PERCENT whose ends are the table's own, not 0..100.
    //
    // `Param::percent` maps `Linear { 0, 100 }` flat, which is right for
    // a fraction and wrong for anything bipolar: the reverb's width
    // shipped once with a 150 % range clamped to 100 by exactly this,
    // and depth and pan here run −1..1.
    let pct = |name: &'static str| {
        with(Param::new(
            name,
            Mapping::Linear {
                min: def.min * 100.0,
                max: def.max * 100.0,
            },
            Unit::Percent,
        ))
    };
    let linear = |name: &'static str, unit| {
        with(Param::new(
            name,
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            unit,
        ))
    };
    match param {
        // --- what a note MEANS, and where it reads -------------------
        sp::MODE => with(Param::choice("mode", sp::MODE_NAMES)),
        sp::REVERSE => with(Param::choice("rev", sp::OFF_ON_NAMES)),
        sp::LOOP_MODE => with(Param::choice("loop", sp::LOOP_NAMES)),
        sp::SLICE_SOURCE => with(Param::choice("slice", sp::SLICE_SOURCE_NAMES)),
        sp::CHOKE => with(Param::choice("choke", sp::OFF_ON_NAMES)),
        sp::FILT_MODE => with(Param::choice("filter", sp::FILT_NAMES)),
        sp::MOD_DEST => with(Param::choice("dest", sp::DEST_NAMES)),
        // A COUNT, not a sweep: 16.4 slices describes something that
        // cannot exist. `Steps` runs 0..count-1, so the count — which
        // starts at 1 — is shifted by `shown`/`natural` rather than by
        // inventing a second mapping.
        sp::SLICES => with(Param::new(
            "slices",
            Mapping::Steps {
                count: def.max as u32,
            },
            Unit::Plain,
        )),
        sp::SLICE => with(Param::new(
            "slice",
            Mapping::Steps {
                count: def.max as u32,
            },
            Unit::Plain,
        )),
        // A MIDI note prints as a name. Sixty is C4, which is what the
        // piano roll already draws.
        sp::ROOT => with(Param::new(
            "root",
            Mapping::Steps {
                count: def.max as u32 + 1,
            },
            Unit::Note,
        )),

        // --- positions in the file, as percent ------------------------
        //
        // Percent rather than frames or seconds, and deliberately: these
        // are FRACTIONS of whatever file is loaded, so they survive a
        // sample being swapped underneath them. A reading in seconds
        // would be a lie about a different file the moment one was
        // dropped.
        sp::START => pct("start"),
        sp::END => pct("end"),
        sp::LOOP_START => pct("loop at"),

        // --- times ----------------------------------------------------
        sp::FADE_IN => log("fade in", Unit::Ms),
        sp::FADE_OUT => log("fade out", Unit::Ms),
        sp::LOOP_XFADE => log("xfade", Unit::Ms),
        sp::AMP_A => log("attack", Unit::Ms),
        sp::AMP_D => log("decay", Unit::Ms),
        sp::AMP_R => log("release", Unit::Ms),
        sp::MOD_A => log("m atk", Unit::Ms),
        sp::MOD_D => log("m dec", Unit::Ms),
        sp::MOD_R => log("m rel", Unit::Ms),

        // --- pitch ----------------------------------------------------
        sp::TUNE => with(linear("tune", Unit::Semitones).bipolar()),
        // Cents are a plain number: `+35 st` would be a lie and there is
        // no cent unit worth adding for one control.
        sp::FINE => with(linear("fine", Unit::Plain).bipolar()),

        // --- the filter ------------------------------------------------
        sp::CUTOFF => log("cutoff", Unit::Hz),
        sp::RES => log("res", Unit::Plain),
        sp::KEYTRACK => pct("keytrk"),

        // --- amounts ---------------------------------------------------
        sp::AMP_S => pct("sustain"),
        sp::MOD_S => pct("m sus"),
        sp::MOD_DEPTH => pct("depth").bipolar(),
        sp::VELOCITY => pct("vel"),
        sp::DRIVE => pct("drive"),
        sp::PREAMP => pct("preamp"),
        sp::PAN => pct("pan").bipolar(),

        // --- the converter ----------------------------------------------
        sp::RATE => log("rate", Unit::Hz),
        // Continuous on purpose — see the table. Two decimals would be
        // noise; one says what a fractional bit depth is doing.
        sp::BITS => with(Param::new(
            "bits",
            Mapping::Linear {
                min: def.min,
                max: def.max,
            },
            Unit::Plain,
        )),
        _ => with(Param::db("gain", def.min, def.max).bipolar()),
    }
}

/// What the widget SHOWS for an engine value.
fn shown(param: u32, value: f32) -> f32 {
    if sp::extra_percent(param) {
        return value * 100.0;
    }
    // A zero-stop knob shows its floor for zero, so the position lands at
    // the very bottom of the travel rather than off the scale entirely.
    if has_zero_stop(param) {
        return value.max(TIME_FLOOR_MS);
    }
    match param {
        // The percent family: the engine keeps `0..=1`, the card prints
        // `0..=100`.
        sp::START
        | sp::END
        | sp::LOOP_START
        | sp::KEYTRACK
        | sp::AMP_S
        | sp::MOD_S
        | sp::MOD_DEPTH
        | sp::VELOCITY
        | sp::DRIVE
        | sp::PREAMP
        | sp::PAN => value * 100.0,
        // One slice is step zero.
        sp::SLICES | sp::SLICE => value - 1.0,
        _ => value,
    }
}

/// What the ENGINE receives for a shown value, clamped through the table
/// so a knob at either stop cannot emit a letter the engine has to bin.
fn natural(param: u32, value: f32) -> f32 {
    if param >= sp::PLAYBACK {
        let v = if sp::extra_percent(param) {
            value / 100.0
        } else if !sp::extra_choices(param).is_empty()
            || matches!(param, sp::SEED | sp::VOICE_COUNT)
        {
            value.round()
        } else {
            value
        };
        return params::def(sp::TABLE, param).clamp(v);
    }
    // ...and the bottom of the travel reports an exact zero on the way
    // back, which is the half that makes the round trip hold.
    if has_zero_stop(param) {
        let raw = if value <= TIME_FLOOR_MS * 1.001 {
            0.0
        } else {
            value
        };
        return params::def(sp::TABLE, param).clamp(raw);
    }
    let raw = match param {
        sp::START
        | sp::END
        | sp::LOOP_START
        | sp::KEYTRACK
        | sp::AMP_S
        | sp::MOD_S
        | sp::MOD_DEPTH
        | sp::VELOCITY
        | sp::DRIVE
        | sp::PREAMP
        | sp::PAN => value / 100.0,
        sp::SLICES | sp::SLICE => value.round() + 1.0,
        sp::ROOT
        | sp::MODE
        | sp::REVERSE
        | sp::LOOP_MODE
        | sp::SLICE_SOURCE
        | sp::CHOKE
        | sp::FILT_MODE
        | sp::MOD_DEST => value.round(),
        _ => value,
    };
    params::def(sp::TABLE, param).clamp(raw)
}

/// The engine-facing value at a normalized knob position.
pub fn sampler_value(param: u32, norm: f32) -> f32 {
    natural(param, param_of(param).value(norm))
}

/// The inverse of [`sampler_value`], for a state stored in engine units.
pub fn sampler_norm(param: u32, value: f32) -> f32 {
    param_of(param).mapping.to_norm(shown(param, value))
}

/// A value in the parameter's OWN words: `slice`, `12.0 bits`, `C4`.
///
/// What the parameter-lock editor prints, so a lock reads the way the
/// knob does rather than as a bare float.
pub fn sampler_format(param: u32, value: f32) -> String {
    let p = param_of(param);
    p.unit.format(shown(param, value))
}

/// How many discrete choices a parameter has — 0 for a continuous one.
/// What a per-choice stepper needs to know, exported rather than
/// re-derived from the ranges.
pub fn sampler_choices(param: u32) -> u32 {
    param_of(param).choices().unwrap_or(0)
}

/// Whether a parameter snaps to named settings rather than sweeping.
pub fn sampler_is_discrete(param: u32) -> bool {
    switch::is_discrete(&param_of(param))
}

/// Whether a parameter lives on a LOG scale — so a modulation sweep of
/// the cutoff moves in ratios exactly where the knob does.
pub fn sampler_is_log(param: u32) -> bool {
    matches!(param_of(param).mapping, Mapping::Log { .. })
}

/// Every parameter as an edit, for a reset, a preset recall, or the
/// moment the device is first loaded.
pub fn sampler_edits(state: &SamplerUi) -> Vec<ParamEdit> {
    sp::TABLE
        .iter()
        .map(|def| ParamEdit {
            param: def.id,
            value: sampler_value(def.id, state.get(def.id)),
        })
        .collect()
}

// -------------------------------------------------------------- layout ---

/// The five pages, and the rows of cells each one puts in reach.
///
/// Grouped the way you WORK rather than the way the signal flows: where
/// the sound comes from, how it repeats, what shapes it, what moves it,
/// what dirties it. The order inside a row is the order you would set
/// them in.
/// How many pages this card has, for whoever needs to keep clear of its
/// tab dots — see `card::tabs_width`. Derived from `PAGES` rather than
/// stated again, so the two cannot disagree.
pub fn pages() -> usize {
    PAGES.len()
}

const PAGES: [&[&[u32]]; 11] = [
    // sample: what plays, and at what pitch.
    &[&[
        sp::MODE,
        sp::START,
        sp::END,
        sp::REVERSE,
        sp::ROOT,
        sp::FADE_IN,
        sp::FADE_OUT,
        sp::TUNE,
        sp::FINE,
        sp::SLICE,
    ]],
    // loop: how it repeats, and how it is cut up.
    &[&[
        sp::LOOP_MODE,
        sp::LOOP_START,
        sp::LOOP_XFADE,
        sp::SLICES,
        sp::SLICE_SOURCE,
        sp::CHOKE,
    ]],
    // shape: the amp envelope and the one filter.
    &[&[
        sp::AMP_A,
        sp::AMP_D,
        sp::AMP_S,
        sp::AMP_R,
        sp::FILT_MODE,
        sp::CUTOFF,
        sp::RES,
        sp::KEYTRACK,
    ]],
    // mod: the one envelope, and where it goes.
    &[&[
        sp::MOD_A,
        sp::MOD_D,
        sp::MOD_S,
        sp::MOD_R,
        sp::MOD_DEST,
        sp::MOD_DEPTH,
        sp::VELOCITY,
    ]],
    // dirt: the three colour stages, and the output.
    &[&[sp::DRIVE, sp::RATE, sp::BITS, sp::PREAMP, sp::GAIN, sp::PAN]],
    &[&[sp::PLAYBACK, sp::TIME, sp::SPEED, sp::WINDOW, sp::TRANSIENT]],
    &[&[
        sp::LOOP_SIZE,
        sp::LOOP_FADE,
        sp::SCAN,
        sp::TRAVEL,
        sp::MOTION_MODE,
        sp::LOOP_UNITS,
        sp::LOOP_EXIT,
    ]],
    &[&[
        sp::PLAY_MODE,
        sp::VOICE_COUNT,
        sp::GLIDE,
        sp::SPREAD,
        sp::ENV_PITCH,
        sp::ENV_POSITION,
        sp::ENV_SIZE,
    ]],
    &[&[
        sp::ENV_FILTER,
        sp::START_JITTER,
        sp::PITCH_JITTER,
        sp::SEED,
        sp::SLIP,
        sp::ATTACK_SHAPE,
        sp::FILTER_SLOPE,
    ]],
    &[&[sp::COMB_FOCUS, sp::COMB_FEED, sp::COMB_DAMP, sp::COMB_MIX]],
    &[&[
        sp::SOURCE_BEATS,
        sp::FIT_BEATS,
        sp::SLICE_THRU,
        sp::HARD,
        sp::SENSE,
        sp::MIN_GAP,
    ]],
];

/// How many `POLY_CELL_H` units one row of labelled cells needs.
///
/// TWO. A labelled cell prints its value on one line and its name on the
/// next, so a row given one unit prints them through each other. See
/// `notes/20260827-device-card-layout.md`, rule 1 — this is the mistake
/// the kick's card made three times.
const CELL_UNITS: usize = 2;

/// Rows of cells on a page. ONE, and the same one on every page.
///
/// One because the WAVEFORM is what this device is edited through, and a
/// second row of cells costs it forty-eight points — better than a third
/// of the picture — to say things a single row says just as well. The
/// layout note's own advice: prefer one row where the cells fit, and nine
/// of them fit (`glue.rs` runs nine and simply declares a wide face).
///
/// The same count on every page because a card whose hero grows and
/// shrinks as you change tabs is a card that jumps.
const ROWS_PER_PAGE: usize = 1;

/// How many rows of height the footer reserves.
const FOOTER_ROWS: usize = ROWS_PER_PAGE * CELL_UNITS;

/// The NARROWEST a cell may be drawn: room for the widest thing it will
/// ever print, and nothing over.
fn cell_min_width(ui: &egui::Ui, theme: &Theme, param: &Param) -> f32 {
    let value = metrics::mono_w(ui, &param.widest_text(), font::VALUE);
    let name = metrics::text_w(ui, &param.name.to_uppercase(), font::MINI_LABEL);
    value.max(name) + theme.sp(space::XS) * 2.0
}

/// The width the whole face needs: its widest row on ANY page, at its
/// narrowest.
///
/// Across every page rather than the open one, and that is the point: a
/// card that resized when you changed tabs would shove the whole rack
/// sideways every time you looked at a different section.
fn face_width(ui: &egui::Ui, theme: &Theme) -> f32 {
    PAGES
        .iter()
        .flat_map(|page| page.iter())
        .map(|row| {
            let cells: f32 = row
                .iter()
                .map(|id| cell_min_width(ui, theme, &param_of(*id)))
                .sum();
            cells + ui.spacing().item_spacing.x * row.len().saturating_sub(1) as f32
        })
        .fold(0.0, f32::max)
}

/// The card's title: the device, then the open page, so five anonymous
/// dots have a name beside them.
fn title(page: usize) -> String {
    let name = sp::PAGES.get(page).copied().unwrap_or("sample");
    format!("sampler · {name}")
}

/// Draw the sampler card. Returns everything it has to say.
pub fn sampler_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut SamplerUi,
    page: u8,
    zoom: f32,
    scroll: f32,
    view: &SamplerView<'_>,
) -> SamplerOutcome {
    let mut out = SamplerOutcome {
        page,
        zoom: clamp_zoom(zoom),
        scroll,
        ..Default::default()
    };
    let mut open = usize::from(page).min(PAGES.len() - 1);
    let layout = Wells::new().compact().row([Well::one()
        .fits(Footprint::new(face_width(ui, theme), 0.0))
        .filling()]);

    card::tabbed_card_sized(
        ui,
        theme,
        &title(open),
        control::DEVICE_TALL_H,
        PAGES.len(),
        &mut open,
        |ui, page| {
            card::wells(ui, theme, &layout, |ui, _| {
                poly_widgets::dark_curve_panel(
                    ui,
                    theme,
                    None,
                    0.0,
                    0,
                    FOOTER_ROWS,
                    |ui, region| match region {
                        poly_widgets::CurveRegion::Plot => {
                            plot(ui, theme, state, view, &mut out, false);
                        }
                        poly_widgets::CurveRegion::Footer => {
                            footer(ui, theme, state, page, &mut out.edits);
                        }
                        poly_widgets::CurveRegion::Header => {}
                    },
                );
            });
        },
    );
    out.page = open as u8;
    out
}

/// The sampler's display at FULL SIZE, for work the card is too small
/// for: placing a marker inside a hi-hat, or finding the exact zero
/// crossing a loop wants.
///
/// The same plot and the same value strip the card draws — not a second
/// editor. A second editor is a second set of gestures to learn and a
/// second place for a bug to live; this is the first one, given room.
///
/// The caller owns the rect and the chrome around it. All this does is
/// fill what it is given.
pub fn sampler_expanded(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut SamplerUi,
    page: u8,
    zoom: f32,
    scroll: f32,
    view: &SamplerView<'_>,
) -> SamplerOutcome {
    let mut out = SamplerOutcome {
        page,
        zoom: clamp_zoom(zoom),
        scroll,
        ..Default::default()
    };
    let mut open = usize::from(page).min(PAGES.len() - 1);

    // The tab strip, spelled out rather than dotted. There is room here
    // for the words, and a page you can NAME is a page you can go back
    // to — the card's five anonymous dots are a concession to width that
    // this view does not have to make.
    ui.horizontal(|ui| {
        for (index, name) in sp::PAGES.iter().enumerate() {
            let picked = index == open;
            if ui.selectable_label(picked, *name).clicked() {
                open = index;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(COLLAPSE_GLYPH)
                .on_hover_text("back to the rack (Esc)")
                .clicked()
            {
                out.collapse = true;
            }
            if !view.name.is_empty() {
                ui.label(
                    egui::RichText::new(view.name)
                        .size(font::MINI_LABEL)
                        .color(theme.text_muted),
                );
            }
        });
    });
    out.page = open as u8;

    poly_widgets::dark_curve_panel(
        ui,
        theme,
        None,
        0.0,
        0,
        FOOTER_ROWS,
        |ui, region| match region {
            poly_widgets::CurveRegion::Plot => plot(ui, theme, state, view, &mut out, true),
            poly_widgets::CurveRegion::Footer => {
                footer(ui, theme, state, usize::from(out.page), &mut out.edits);
            }
            poly_widgets::CurveRegion::Header => {}
        },
    );
    out
}

/// The value strip for one page.
fn footer(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut SamplerUi,
    page: usize,
    edits: &mut Vec<ParamEdit>,
) {
    let Some(rows) = PAGES.get(page) else {
        return;
    };
    // The row height the PANEL reserved, with the gaps taken off FIRST —
    // dividing the whole height by the row count hands each row a share
    // of the gaps as well and walks them down the card until they
    // overlap. Layout note, rule 2.
    let gap = theme.sp(space::XXS);
    let count = rows.len().max(1) as f32;
    let height = ((ui.available_height() - gap * (count - 1.0)) / count).max(1.0);
    ui.spacing_mut().item_spacing.y = gap;
    for row in rows.iter() {
        ui.horizontal(|ui| {
            let gap_x = ui.spacing().item_spacing.x;
            for (drawn, param) in row.iter().enumerate() {
                let spec = param_of(*param);
                // The share is recomputed from what is ACTUALLY left,
                // cell by cell, rather than divided up in advance.
                // Worked out ahead, any cell needing more than its share
                // spends the row's remainder and the last one is pushed
                // off the card's right edge. Layout note, rule 4.
                let left = (row.len() - drawn) as f32;
                let room = ui.available_width() - gap_x * (left - 1.0).max(0.0);
                let width = (room / left).floor().max(1.0);
                let discrete = sampler_is_discrete(*param);
                let Some(norm) = state.slot(*param) else {
                    continue;
                };
                ui.allocate_ui_with_layout(
                    egui::vec2(width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        ui.set_height(height);
                        // Counts and choices get the STEPPED cell,
                        // everything else the bar: a mode sweeping
                        // smoothly through values it cannot take would be
                        // a control lying about what it does.
                        let moved = if discrete {
                            poly_widgets::labeled_cell_steps(ui, theme, &spec, norm, None)
                        } else {
                            poly_widgets::labeled_cell_bar(ui, theme, &spec, norm, None)
                        };
                        if moved {
                            edits.push(ParamEdit {
                                param: *param,
                                value: sampler_value(*param, *norm),
                            });
                        }
                    },
                );
            }
        });
    }
}

// ---------------------------------------------------------------- plot ---

/// The waveform, the region, the loop and the slices.
///
/// Every handle lands through [`SamplerOutcome`] — see the module header.
/// The furthest the display will zoom in. Sixty-four times over a
/// five-minute file is about five seconds across the plot, which is close
/// enough to place a marker inside a hi-hat and far enough that the
/// waveform still has columns to draw.
pub const ZOOM_MAX: f32 = 64.0;

fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(1.0, ZOOM_MAX)
    } else {
        1.0
    }
}

/// The window the plot is showing, as `(left, span)` fractions of the
/// whole file. Always inside `0..=1`, and always the full width at 1x —
/// so a display that is not zoomed cannot be scrolled off its own
/// material.
fn window(zoom: f32, scroll: f32) -> (f32, f32) {
    let span = 1.0 / clamp_zoom(zoom);
    let left = if scroll.is_finite() {
        scroll.clamp(0.0, 1.0 - span)
    } else {
        0.0
    };
    (left, span)
}

/// The width of a draggable handle, in points.
const HANDLE_W: f32 = 6.0;

/// The fraction of the handle height that is the grab zone above and
/// below the plot — so a handle at the very edge of the file is still
/// draggable.
const HANDLE_PAD: f32 = 4.0;

fn plot(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &SamplerUi,
    view: &SamplerView<'_>,
    out: &mut SamplerOutcome,
    expanded: bool,
) {
    let edits = &mut out.edits;
    let rect = ui.available_rect_before_wrap();
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter_at(rect);

    if view.frames == 0 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "drop a sample",
            egui::FontId::proportional(font::MINI_LABEL),
            theme.text_muted,
        );
        return;
    }

    let start = sampler_value(sp::START, state.get(sp::START));
    let end = sampler_value(sp::END, state.get(sp::END));
    let (lo, hi) = if end > start {
        (start, end)
    } else {
        (start, 1.0)
    };

    // The plot body: allocated FIRST so the handles win where they
    // overlap (contract rule 1 — egui gives a press to the last widget
    // added at that position).
    let body_id = ui.id().with("body");
    let body = ui
        .interact(rect, body_id, egui::Sense::click_and_drag())
        .affords(Affords::Steer);

    // The window as it stands coming into the frame. The gestures below
    // move it; everything that DRAWS re-reads it afterwards.
    let (left, span) = window(out.zoom, out.scroll);

    // ZOOM, about the pointer. Zooming about the CENTRE is the easy
    // version and the wrong one: the thing you are pointing at is the
    // thing you want to keep, and a centre zoom slides it away as you go
    // in. Anchoring on the pointer means the sample under the cursor
    // stays under the cursor.
    if body.hovered() {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            let anchor = ui.ctx().pointer_latest_pos().map_or(0.5, |at| {
                ((at.x - rect.left()) / rect.width()).clamp(0.0, 1.0)
            });
            let held = left + anchor * span;
            // A notch is about 50 raw pixels, and this puts one notch at
            // roughly a 1.5x step — brisk enough to cross a whole file in
            // a few flicks, gentle enough to stop where you meant.
            let next = clamp_zoom(out.zoom * (wheel / 120.0).exp2());
            let next_span = 1.0 / next;
            out.zoom = next;
            out.scroll = (held - anchor * next_span).clamp(0.0, 1.0 - next_span);
            // Consume it: a wheel spent here must not ALSO scroll the rack
            // the card sits in. The same courtesy `device::adjust` pays.
            ui.ctx().input_mut(|i| i.smooth_scroll_delta.y = 0.0);
        }
    }

    // PAN, by dragging the body. Only meaningful once zoomed, and the
    // cursor says so — a grab hand over a display that cannot move would
    // be a promise the card does not keep.
    if span < 1.0 {
        if body.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        if body.dragged() {
            let moved = body.drag_delta().x / rect.width() * span;
            out.scroll = (out.scroll - moved).clamp(0.0, 1.0 - span);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }
    // The window, AFTER the gestures — everything below must draw where
    // the display is now rather than where it was when the frame began,
    // or a zoom reads as one frame of sludge.
    //
    // `at` may return a point OUTSIDE the rect, and that is load-bearing:
    // a marker just off the left edge has to draw off the left edge
    // rather than pile up on it.
    let (left, span) = window(out.zoom, out.scroll);
    let at = |fraction: f32| rect.left() + (fraction - left) / span * rect.width();
    let fraction_at = |x: f32| left + ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) * span;

    // Everything OUTSIDE the played region is dimmed rather than hidden.
    // A sampler's start and end are a window onto a file, and a window
    // you cannot see past is one you cannot aim.
    for span in [(0.0, lo), (hi, 1.0)] {
        if span.1 > span.0 {
            painter.rect_filled(
                egui::Rect::from_x_y_ranges(at(span.0)..=at(span.1), rect.y_range()),
                0.0,
                theme.surface_sunken.gamma_multiply(0.5),
            );
        }
    }

    // The waveform: min/max per pixel column with the RMS as a brighter
    // core, which is the only way a quiet passage inside a loud file is
    // legible at this size.
    if !view.wave.is_empty() {
        let columns = (rect.width().ceil() as usize).clamp(2, 2_048);
        let half = rect.height() * 0.5 - stroke::HAIR;
        for column in 0..columns {
            // Across the WINDOW, not across the file: zoomed in, each
            // pixel column is a slimmer slice of the picture, which is
            // what makes zooming show more rather than the same thing
            // bigger.
            let along = left + span * (column as f32 / (columns - 1).max(1) as f32);
            let index =
                ((along * (view.wave.len() - 1) as f32).round() as usize).min(view.wave.len() - 1);
            let Some(bin) = view.wave.get(index) else {
                continue;
            };
            let x = rect.left() + column as f32 + 0.5;
            let inside = along >= lo && along <= hi;
            let colour = if inside {
                theme.role_level
            } else {
                theme.role_level_dim
            };
            painter.line_segment(
                [
                    egui::pos2(x, mid_y(rect) - bin.max.clamp(-1.0, 1.0) * half),
                    egui::pos2(x, mid_y(rect) - bin.min.clamp(-1.0, 1.0) * half),
                ],
                egui::Stroke::new(1.0, colour.gamma_multiply(0.55)),
            );
            painter.line_segment(
                [
                    egui::pos2(x, mid_y(rect) - bin.rms.clamp(0.0, 1.0) * half),
                    egui::pos2(x, mid_y(rect) + bin.rms.clamp(0.0, 1.0) * half),
                ],
                egui::Stroke::new(1.0, colour),
            );
        }
    } else {
        painter.line_segment(
            [
                egui::pos2(rect.left(), mid_y(rect)),
                egui::pos2(rect.right(), mid_y(rect)),
            ],
            egui::Stroke::new(stroke::HAIR, theme.grid_beat),
        );
    }

    // The slice markers, painted under the handles so an edge that lands
    // on one is still visible.
    //
    // Drawn in EVERY mode, not just slice mode. Where the cuts fall is
    // something you decide BEFORE committing to playing them, and a
    // picture that only appears once you have already switched is a
    // picture that arrives after the decision it was for. Outside slice
    // mode they are dimmed — present, not shouting.
    if view.slices.len() > 1 {
        let slicing = sampler_value(sp::MODE, state.get(sp::MODE)).round() == sp::MODE_SLICE;
        let line = if slicing {
            theme.role_shape
        } else {
            theme.role_shape.gamma_multiply(0.45)
        };
        let frames = view.frames.max(1) as f32;

        // Alternating bands first, underneath everything. Two adjacent
        // slices of similar material look like one slice with a line
        // through it; banding is what makes the COUNT readable at a
        // glance, which is the thing you are actually judging when you
        // turn the slices knob.
        if slicing {
            for (index, pair) in view.slices.windows(2).enumerate() {
                if index % 2 == 1 {
                    continue;
                }
                painter.rect_filled(
                    egui::Rect::from_x_y_ranges(
                        at(pair[0] as f32 / frames)..=at(pair[1] as f32 / frames),
                        rect.y_range(),
                    ),
                    0.0,
                    theme.role_shape.gamma_multiply(0.08),
                );
            }
            // The last band has no pair to close it.
            if view.slices.len() % 2 == 1
                && let Some(last) = view.slices.last()
            {
                painter.rect_filled(
                    egui::Rect::from_x_y_ranges(
                        at(*last as f32 / frames)..=rect.right(),
                        rect.y_range(),
                    ),
                    0.0,
                    theme.role_shape.gamma_multiply(0.08),
                );
            }
        }

        for boundary in view.slices.iter().skip(1) {
            let x = at(*boundary as f32 / frames);
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(stroke::HAIR, line),
            );
        }

        // The NOTE each slice answers to, where there is room to print it.
        //
        // A number nobody can map to a key is decoration; these are the
        // actual notes, so the label is what you play rather than an
        // index into a list you cannot see. Skipped entirely once the
        // bands are narrower than the text, because a row of overlapping
        // numerals is worse than no numerals.
        let width_per = rect.width() / (view.slices.len().max(1) as f32);
        if slicing && width_per > theme.sp(space::MD) * 2.0 {
            for (index, boundary) in view.slices.iter().enumerate() {
                let Some(pitch) = u8::try_from(index)
                    .ok()
                    .and_then(|i| sp::SLICE_BASE_NOTE.checked_add(i))
                    .filter(|p| *p <= 127)
                else {
                    break;
                };
                painter.text(
                    egui::pos2(
                        at(*boundary as f32 / frames) + theme.sp(space::XXS),
                        rect.top() + theme.sp(space::XXS),
                    ),
                    egui::Align2::LEFT_TOP,
                    Unit::Note.format(f32::from(pitch)),
                    egui::FontId::proportional(font::MINI_LABEL),
                    theme.text_muted,
                );
            }
        }
    }

    // The loop start, where a loop is running.
    if sampler_value(sp::LOOP_MODE, state.get(sp::LOOP_MODE)).round() != sp::LOOP_OFF {
        let loop_at = sampler_value(sp::LOOP_START, state.get(sp::LOOP_START));
        let x = at(lo + (hi - lo) * loop_at);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(stroke::BOLD, theme.role_time),
        );
    }

    // The region edges.
    for (x, colour) in [(at(lo), theme.accent), (at(hi), theme.accent)] {
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(stroke::BOLD, colour),
        );
    }

    // The sounding voices. One line each, and it is what makes a chopped
    // break legible while it plays.
    for position in view.voices {
        let x = at(*position);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(stroke::HAIR, theme.playhead),
        );
    }

    // What the file IS, bottom left, and the cap if it bit.
    let mut note = view.name.to_owned();
    if view.truncated {
        note.push_str("  · cut to 5 min");
    }
    if view.original_rate != 0 && view.original_rate != 48_000 {
        note.push_str(&format!("  · {} k", view.original_rate / 1_000));
    }
    if !note.is_empty() {
        painter.text(
            rect.left_bottom() + egui::vec2(theme.sp(space::XS), -theme.sp(space::XS)),
            egui::Align2::LEFT_BOTTOM,
            note,
            egui::FontId::proportional(font::MINI_LABEL),
            theme.text_muted,
        );
    }

    // ---- draggable handles, allocated AFTER painting so they win
    // where they overlap the body (contract rule 1) ----

    let frame_w = view.frames.max(1) as f32;
    let h_rect = |fraction: f32| {
        let x = at(fraction);
        egui::Rect::from_min_size(
            egui::pos2(x - HANDLE_W * 0.5, rect.top() - HANDLE_PAD),
            egui::vec2(HANDLE_W, rect.height() + HANDLE_PAD * 2.0),
        )
    };

    // START and END are fractions OF THE FILE, so the pointer's position
    // in the window is the value directly.
    handle_param(
        ui,
        theme,
        Handle {
            param: sp::START,
            rect: h_rect(start),
            id: body_id.with("start"),
            current: start,
            value_at: &|fraction| fraction,
        },
        edits,
        &fraction_at,
    );
    handle_param(
        ui,
        theme,
        Handle {
            param: sp::END,
            rect: h_rect(end),
            id: body_id.with("end"),
            current: end,
            value_at: &|fraction| fraction,
        },
        edits,
        &fraction_at,
    );

    // LOOP_START is a fraction of the REGION, not of the file, so it
    // needs the region divided back out. Only drawn while a loop is
    // running — a handle for something switched off is a handle that lies.
    if sampler_value(sp::LOOP_MODE, state.get(sp::LOOP_MODE)).round() != sp::LOOP_OFF {
        let loop_val = sampler_value(sp::LOOP_START, state.get(sp::LOOP_START));
        let width = (hi - lo).max(f32::EPSILON);
        handle_param(
            ui,
            theme,
            Handle {
                param: sp::LOOP_START,
                rect: h_rect(lo + width * loop_val),
                id: body_id.with("loop_start"),
                current: loop_val,
                value_at: &|fraction| (fraction - lo) / width,
            },
            edits,
            &fraction_at,
        );
    }

    // Slice marker handles.
    //
    // In EVERY mode, not just slice mode — for the same reason the
    // markers are drawn in every mode. Where the cuts fall is something
    // you set up before you commit to playing them, and a marker you can
    // see but cannot move is worse than one you cannot see at all.
    //
    // Marker zero is skipped: it is the start of the file, it is not a
    // cut, and dragging it would mean something else entirely.
    for (idx, boundary) in view.slices.iter().enumerate().skip(1) {
        let fraction = *boundary as f32 / frame_w;
        // Off the window's edge means off the card. Zoomed in, most
        // markers are, and allocating interactions for them would put
        // invisible grab zones along both edges of the plot.
        if fraction < left || fraction > left + span {
            continue;
        }
        let h_rect = h_rect(fraction);
        let id = body_id.with(("slice", idx));
        let response = ui
            .interact(h_rect, id, egui::Sense::click_and_drag())
            .affords(Affords::Sweep);
        if response.hovered() || response.is_pointer_button_down_on() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            painter.line_segment(
                [
                    egui::pos2(h_rect.center().x, rect.top()),
                    egui::pos2(h_rect.center().x, rect.bottom()),
                ],
                egui::Stroke::new(stroke::BOLD, theme.role_shape),
            );
        }
        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            // Through the window's own mapping, so a drag at 32x moves
            // the marker by what the pointer covered on SCREEN and not by
            // a thirty-second of the file per pixel.
            let new_frame = (fraction_at(pos.x) * frame_w) as u64;
            out.slice_moved = Some((idx, new_frame));
        }
    }

    // The expand button, allocated LAST so it wins its own corner against
    // any handle that happens to be under it — egui gives a press to the
    // last widget added at a position, and a marker at the very end of
    // the file sits exactly here.
    if !expanded && corner_button(ui, theme, rect, EXPAND_GLYPH, "edit full size") {
        out.expand = true;
    }
}

/// The glyph on the button that makes the display full size, and the one
/// that puts it back.
const EXPAND_GLYPH: &str = "⤢";
const COLLAPSE_GLYPH: &str = "⤡";

/// A small square button in a region's top-right corner. Returns whether
/// it was clicked this frame.
fn corner_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    within: egui::Rect,
    glyph: &str,
    tip: &str,
) -> bool {
    let side = theme.sp(control::POLY_CELL_H);
    let pad = theme.sp(space::XXS);
    let rect = egui::Rect::from_min_size(
        egui::pos2(within.right() - side - pad, within.top() + pad),
        egui::vec2(side, side),
    );
    if rect.width() <= 0.0 || !within.contains(rect.min) {
        return false;
    }
    let response = ui
        .interact(rect, ui.id().with(("corner", glyph)), egui::Sense::click())
        .affords(Affords::Press);
    let hot = response.hovered();
    ui.painter().rect_filled(
        rect,
        theme.sp(space::XXS),
        if hot {
            theme.accent_muted
        } else {
            theme.surface_sunken.gamma_multiply(0.7)
        },
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(font::LABEL),
        if hot { theme.accent } else { theme.text_muted },
    );
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.on_hover_text(tip).clicked()
}

/// The vertical centre of the plot region.
fn mid_y(rect: egui::Rect) -> f32 {
    rect.center().y
}

/// Draw a draggable handle for a continuous parameter (start, end, loop
/// start). Each gets its own `ui.interact` with its own id — contract
/// rule 1, no nearest-handle search anywhere.
///
/// `plot_rect` is the plot region the handle sits in, so the pointer
/// position can be converted to a fraction of the file.
/// One draggable parameter handle on the plot.
///
/// A struct rather than eight positional arguments, and `value_at` is why
/// it needs one: the three handles sit on the same axis but do not mean
/// the same thing along it. Start and end are fractions of the FILE;
/// loop start is a fraction of the REGION. Handing each one the
/// conversion from "where the pointer is in the window" to "what this
/// parameter should read" is what stops that difference from being
/// re-derived, differently, at three call sites.
struct Handle<'a> {
    param: u32,
    rect: egui::Rect,
    id: egui::Id,
    /// The parameter's value NOW, in the units `value_at` returns.
    current: f32,
    value_at: &'a dyn Fn(f32) -> f32,
}

/// Draw and drive one handle. Its own `ui.interact`, its own id —
/// contract rule 1, so a drag that began here stays here until the button
/// comes up, past its neighbours and past the ends of its range.
fn handle_param(
    ui: &mut egui::Ui,
    theme: &Theme,
    handle: Handle<'_>,
    edits: &mut Vec<ParamEdit>,
    fraction_at: &dyn Fn(f32) -> f32,
) {
    let response = ui
        .interact(handle.rect, handle.id, egui::Sense::click_and_drag())
        .affords(Affords::Slide);
    let painter = ui.painter();
    let top = handle.rect.top() + HANDLE_PAD;
    let bottom = handle.rect.bottom() - HANDLE_PAD;

    let active = response.hovered() || response.is_pointer_button_down_on();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let colour = if active {
        theme.accent
    } else {
        theme.accent.gamma_multiply(0.6)
    };
    painter.line_segment(
        [
            egui::pos2(handle.rect.center().x, top),
            egui::pos2(handle.rect.center().x, bottom),
        ],
        egui::Stroke::new(stroke::BOLD, colour),
    );

    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        // Through the WINDOW's mapping: zoomed in, a pixel is a smaller
        // piece of the file, and a handle that ignored that would fly
        // across the whole sample on a short drag.
        let value = (handle.value_at)(fraction_at(pos.x)).clamp(0.0, 1.0);
        if (value - handle.current).abs() > f32::EPSILON {
            edits.push(ParamEdit {
                param: handle.param,
                // The value IS the engine value here: all three of these
                // parameters are plain `0..=1` fractions in the table.
                value: crate::params::def(sp::TABLE, handle.param).clamp(value),
            });
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// The array and the table are the same length. This is the whole
    /// safety of indexing knobs by wire id, and it is one line.
    #[test]
    fn the_state_has_a_slot_for_every_row() {
        assert_eq!(sp::TABLE.len(), ROWS_TOTAL);
        for (i, def) in sp::TABLE.iter().enumerate() {
            assert_eq!(def.id as usize, i, "{} is not at its own index", def.name);
        }
    }

    /// EVERY ROW ROUND-TRIPS between engine units and knob positions. A
    /// mapping that was not its own inverse would walk a value every time
    /// a project was saved and loaded.
    #[test]
    fn value_and_norm_round_trip() {
        for def in sp::TABLE {
            for at in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                let value = sampler_value(def.id, at);
                assert!(
                    value >= def.min - 1e-3 && value <= def.max + 1e-3,
                    "{} at {at} left the table's range: {value}",
                    def.name
                );
                let back = sampler_norm(def.id, value);
                let again = sampler_value(def.id, back);
                assert!(
                    (again - value).abs() <= (value.abs() * 1e-3).max(1e-3),
                    "{} did not round-trip: {value} -> {back} -> {again}",
                    def.name
                );
            }
        }
    }

    /// And the ends reach the ends: a knob at either stop must be able to
    /// ask for the table's own limit, or part of the range is unreachable.
    #[test]
    fn every_position_is_a_legal_engine_value() {
        for def in sp::TABLE {
            let bottom = sampler_value(def.id, 0.0);
            let top = sampler_value(def.id, 1.0);
            assert!(
                (bottom - def.min).abs() <= (def.min.abs() * 1e-3).max(1e-3),
                "{} bottoms out at {bottom}, table says {}",
                def.name,
                def.min
            );
            assert!(
                (top - def.max).abs() <= (def.max.abs() * 1e-3).max(1e-3),
                "{} tops out at {top}, table says {}",
                def.name,
                def.max
            );
        }
    }

    /// The card's defaults ARE the table's defaults, so a fresh sampler
    /// on screen is the fresh sampler the engine builds.
    #[test]
    fn defaults_come_from_the_table() {
        let ui = SamplerUi::default();
        for def in sp::TABLE {
            let shown = sampler_value(def.id, ui.get(def.id));
            assert!(
                (shown - def.default).abs() <= (def.default.abs() * 1e-3).max(1e-3),
                "{} defaults to {shown}, table says {}",
                def.name,
                def.default
            );
        }
    }

    /// Every row is REACHABLE from some page. A row missing from the
    /// layout is a parameter nobody can turn.
    #[test]
    fn every_table_row_leaves_as_an_edit() {
        let mut seen: Vec<u32> = PAGES
            .iter()
            .flat_map(|page| page.iter())
            .flat_map(|row| row.iter().copied())
            .collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a parameter is on two pages");

        let mut want: Vec<u32> = sp::TABLE.iter().map(|d| d.id).collect();
        want.sort_unstable();
        assert_eq!(seen, want, "the pages and the table disagree");

        // And the edit list covers the table exactly.
        let edits = sampler_edits(&SamplerUi::default());
        let mut ids: Vec<u32> = edits.iter().map(|e| e.param).collect();
        ids.sort_unstable();
        assert_eq!(ids, want);
    }

    /// Every page claims the rows `params::sampler::page_of` says it
    /// does. Two descriptions of one layout, held together.
    #[test]
    fn the_pages_agree_with_the_table() {
        for (page, rows) in PAGES.iter().enumerate() {
            for id in rows.iter().flat_map(|row| row.iter()) {
                assert_eq!(
                    sp::page_of(*id),
                    page,
                    "row {id} is drawn on page {page} and filed under {}",
                    sp::page_of(*id)
                );
            }
        }
    }

    /// Every page has the same number of rows, so the hero does not
    /// change height when you change tabs.
    #[test]
    fn the_footer_reserves_two_lines_for_every_row_of_cells() {
        for (page, rows) in PAGES.iter().enumerate() {
            assert_eq!(
                rows.len(),
                ROWS_PER_PAGE,
                "page {page} has {} rows",
                rows.len()
            );
        }
        assert_eq!(FOOTER_ROWS, ROWS_PER_PAGE * CELL_UNITS);
        // The panel's own arithmetic, restated: what is reserved must be
        // at least what the rows need, and the plot must keep more than
        // three units or it is a line rather than a picture.
        let cell = crate::ui::tokens::control::POLY_CELL_H;
        let gap = crate::ui::tokens::space::XXS;
        let footer = cell * FOOTER_ROWS as f32 + gap * (FOOTER_ROWS as f32 - 1.0);
        let plot = control::DEVICE_TALL_H - footer;
        assert!(
            plot > cell * 3.0,
            "the footer takes {footer} of {}, leaving the plot {plot}",
            control::DEVICE_TALL_H
        );
        // And the HERO keeps most of the card. This device is edited
        // through its waveform, so a second row of cells creeping back in
        // — which is what would break this — costs the picture more than
        // it is worth. Sixty per cent is a floor, not a target: it sits at
        // about eighty.
        assert!(
            plot > control::DEVICE_TALL_H * 0.6,
            "the plot is only {plot} of {}, which is not a hero any more",
            control::DEVICE_TALL_H
        );
    }

    /// A discrete row snaps and a continuous one does not — the two
    /// halves of what `labeled_cell_steps` versus `labeled_cell_bar`
    /// decides.
    #[test]
    fn the_discretes_are_the_ones_that_snap() {
        for id in [
            sp::MODE,
            sp::REVERSE,
            sp::LOOP_MODE,
            sp::SLICE_SOURCE,
            sp::CHOKE,
            sp::FILT_MODE,
            sp::MOD_DEST,
            sp::SLICES,
            sp::ROOT,
        ] {
            assert!(sampler_is_discrete(id), "{id} should snap");
        }
        for id in [sp::START, sp::CUTOFF, sp::BITS, sp::GAIN, sp::PAN] {
            assert!(!sampler_is_discrete(id), "{id} should sweep");
        }
    }

    /// The times and the frequencies are LOG, so a modulation sweep moves
    /// in ratios exactly where the knob does.
    #[test]
    fn the_times_and_frequencies_are_logarithmic() {
        for id in [
            sp::FADE_IN,
            sp::FADE_OUT,
            sp::LOOP_XFADE,
            sp::AMP_A,
            sp::AMP_D,
            sp::AMP_R,
            sp::MOD_A,
            sp::MOD_D,
            sp::MOD_R,
            sp::CUTOFF,
            sp::RATE,
        ] {
            assert!(sampler_is_log(id), "{id} should be logarithmic");
        }
        assert!(!sampler_is_log(sp::PAN));
        assert!(!sampler_is_log(sp::MODE));
    }

    /// The root note prints as a name, and middle C is C4.
    #[test]
    fn the_root_note_prints_as_a_note() {
        let root = param_of(sp::ROOT);
        assert_eq!(root.unit.format(60.0), "C4");
        assert_eq!(root.unit.format(0.0), "C-1");
        assert_eq!(root.unit.format(127.0), "G9");
    }

    /// A slice count of one is step zero, and the count never reads as a
    /// fraction.
    #[test]
    fn the_slice_count_is_a_count() {
        assert_eq!(sampler_value(sp::SLICES, 0.0), 1.0);
        assert_eq!(sampler_value(sp::SLICES, 1.0), sp::SLICES_MAX);
        for at in 0..=20 {
            let v = sampler_value(sp::SLICES, at as f32 / 20.0);
            assert_eq!(v, v.round(), "{v} slices is not a number of slices");
        }
    }
}
