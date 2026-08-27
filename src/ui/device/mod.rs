//! Device widgets: the synth/FX control vocabulary, and the wiring layer
//! that binds a widget to an audio parameter without either knowing the
//! other.
//!
//! # Where this sits
//!
//! `device` is `kit`'s sibling at layer 3 — it may use tokens, theme, vm,
//! action, kit, and egui, and it is the second of exactly two places where
//! custom painting (`ui.painter()`) is allowed. It must never import
//! panels, the host, or the engine (enforced by the layer test in
//! `ui::tests`). Direction between the siblings is one-way: `device` may
//! call `kit`; `kit` must never know `device`.
//!
//! # The wiring
//!
//! Every widget speaks NORMALIZED values (`f32` in `0..=1`) and a
//! [`Param`] descriptor that owns the mapping to the natural unit
//! (Hz, dB, ms, ...) and its formatting. Panels hold the normalized value,
//! hand it to a widget, and return the edit as an action; the app layer
//! translates to engine commands. Widgets never see engine types, and the
//! engine never sees widgets — the `Param` is the entire interface.
//!
//! Normalized-value convention (same reason `kit::meter` takes `norm`):
//! parameter *scaling* is decided once, in the `Param`, instead of once per
//! widget per panel.
//!
//! # What lives here
//!
//! - [`param`]  — `Param`/`Mapping`/`Unit`: the wiring vocabulary.
//! - [`card`]   — the fixed-height, content-width container every device
//!   UI lives in, and the `Wells` layout that divides a card body into
//!   weighted, exactly-tiling sub-panels — `sections` for a plain grid,
//!   `sub_wells` for one more level of grouping inside a well. A chain of
//!   cards is a device rack.
//! - [`bezier`] — pure curve math (no egui): eval, y-at-x, polylines,
//!   bendable segments. Shared by the envelope editor and any future
//!   curve display.
//! - [`metrics`] — `Footprint`: the SIZE CONTRACT every widget publishes,
//!   so a container can reserve room for a control's text before the
//!   control draws. Every widget module exposes a `footprint()` beside its
//!   draw function, and the draw function allocates exactly that.
//! - [`knob`]   — param-aware rotary, unipolar and bipolar.
//! - [`fader`]  — vertical fader and horizontal slider.
//! - [`xy`]     — XY pad over two params.
//! - [`envelope`] — ADSR editor with draggable handles.
//! - [`shaper`]   — waveshaper transfer curve: hard/soft clip, cubic,
//!   wavefold and bitcrush, on linear axes. The one curve where folding
//!   back is a feature rather than a bug.
//! - [`spectrum`] — log-frequency spectrum grid display.
//! - [`dynamics`] — the transfer curve a compressor, limiter, gate or
//!   expander applies, plus the gain-reduction meter that goes beside it.
//!   One widget: a limiter is a compressor with a high ratio, and a gate
//!   is an expander with one.
//! - [`filter`]   — filter response curve: cascaded Butterworth sections
//!   at 6-48 dB/octave, drawn as the DIGITAL response, with a nonlinear
//!   drive model that squashes resonance and floors the stopband.
//! - [`readout`]  — typed value text: static and drag-editable.
//! - [`field`]    — the value you can drag OR type into, with a
//!   unit-aware parser: "250ms", "1.5k", "-6 dB".
//! - [`switch`]   — the segmented DISCRETE control: filter modes, wave
//!   shapes, sync divisions. Every other widget here is continuous.
//! - [`meter`]    — dB-scaled peak meter: instant attack, slow release,
//!   peak hold, latching clip light.
//! - [`synth`]    — the sine synth device card, the first real device.
//! - [`reverb`]   — the reverb device card, the first effect.
//! - [`poly`]     — the workhorse poly synth's single tabbed instrument,
//!   with oscillator, filter, amp and modulation screens.
//! - [`kick`], [`snare`], [`tom`], [`hat`], [`handclap`] — the drum rack.
//!   Each is a card in the same shape: one hero display of the thing that
//!   IS the drum, over a strip of labelled cells. The hero differs
//!   because the drums do — a pitch trajectory for the kick and tom, two
//!   decays for the snare, the oscillator bank and its filter window for
//!   the 808 hat, the burst pattern for the clap.

pub mod adjust;
pub mod bezier;
pub mod card;
pub mod design;
pub mod disperser;
pub mod dynamics;
pub mod echo;
pub mod envelope;
pub mod eq;
pub mod fader;
pub mod field;
pub mod filter;
pub mod glue;
pub mod handclap;
pub mod hat;
pub mod kick;
pub mod knob;
pub mod limiter;
pub mod lofi;
pub mod meter;
pub mod metrics;
pub mod modulato;
pub mod param;
pub mod phaser;
pub mod poly;
pub mod poly_widgets;
/// NOT `#[cfg(test)]`, and deliberately.
///
/// The device UI contract requires a pointer test for every draggable
/// target, and some of those targets live in the BINARY crate — the
/// waveform editor's selection edges, the piano roll. A `cfg(test)`
/// module here is invisible to them: when the binary's tests build, this
/// library is an ordinary dependency compiled without `cfg(test)`, so
/// the contract would be unenforceable exactly where it has already been
/// broken once. Nothing references it outside tests, so it costs a
/// release build nothing.
pub mod probe;
pub mod readout;
pub mod reverb;
pub mod sampler;
pub mod sat;
pub mod scope;
pub mod shaper;
pub mod sheen;
pub mod snare;
pub mod spectrum;
pub mod switch;
pub mod synth;
pub mod tilt;
pub mod tom;
pub mod utility;
pub mod xy;

pub use bezier::{Cubic, Pt};
pub use card::{Well, Wells, card, empty_card, sections, sub_wells, tabbed_card, wells};
pub use disperser::{
    DisperserUi, disperser_card, disperser_edits, disperser_is_discrete, disperser_is_log,
    disperser_norm, disperser_value,
};
pub use echo::{
    EchoUi, echo_card, echo_edits, echo_is_discrete, echo_is_log, echo_norm, echo_value,
};
pub use envelope::Adsr;
pub use eq::{EqUi, eq_card, eq_edits, eq_is_discrete, eq_is_log, eq_norm, eq_value};
pub use filter::{
    FilterUi, filter_card, filter_choices, filter_edits, filter_format, filter_is_discrete,
    filter_is_log, filter_norm, filter_value,
};
pub use glue::{
    GlueUi, glue_card, glue_edits, glue_is_discrete, glue_is_log, glue_norm, glue_value,
};
pub use handclap::{
    HandclapUi, handclap_card, handclap_edits, handclap_is_discrete, handclap_is_log,
    handclap_norm, handclap_value,
};
pub use hat::{HatUi, hat_card, hat_edits, hat_is_discrete, hat_is_log, hat_norm, hat_value};
pub use limiter::{
    LimiterUi, limiter_card, limiter_choices, limiter_edits, limiter_format, limiter_is_discrete,
    limiter_is_log, limiter_norm, limiter_value,
};
pub use lofi::{
    LofiUi, lofi_card, lofi_edits, lofi_is_discrete, lofi_is_log, lofi_norm, lofi_value,
};
pub use metrics::Footprint;
pub use param::{Mapping, Param, Unit};
pub use phaser::{
    PhaserUi, phaser_card, phaser_edits, phaser_is_discrete, phaser_is_log, phaser_norm,
    phaser_value,
};
pub use poly::{
    OscUi, PolyUi, poly_card, poly_choices, poly_edits, poly_format, poly_is_discrete, poly_is_log,
    poly_norm, poly_value,
};
pub use reverb::{ReverbUi, reverb_card, reverb_edits, reverb_norm, reverb_value};
pub use sampler::{
    SamplerOutcome, SamplerUi, SamplerView, WaveColumn, sampler_card, sampler_choices,
    sampler_edits, sampler_expanded, sampler_format, sampler_is_discrete, sampler_is_log,
    sampler_norm, sampler_value,
};
pub use sat::{SatUi, sat_card, sat_edits, sat_is_discrete, sat_is_log, sat_norm, sat_value};
pub use sheen::{
    SheenUi, sheen_card, sheen_edits, sheen_is_discrete, sheen_is_log, sheen_norm, sheen_value,
};
pub use snare::{
    SnareUi, snare_card, snare_edits, snare_is_discrete, snare_is_log, snare_norm, snare_value,
};
pub use synth::{
    ParamEdit, SineSynthUi, sine_synth_card, sine_synth_edits, sine_synth_format,
    sine_synth_is_log, sine_synth_norm, sine_synth_value,
};
pub use tilt::{
    TiltUi, tilt_card, tilt_edits, tilt_is_discrete, tilt_is_log, tilt_norm, tilt_value,
};
pub use tom::{TomUi, tom_card, tom_edits, tom_is_discrete, tom_is_log, tom_norm, tom_value};
pub use utility::{
    UtilityUi, utility_card, utility_edits, utility_is_discrete, utility_is_log, utility_norm,
    utility_value,
};
