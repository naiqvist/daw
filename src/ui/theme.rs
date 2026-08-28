//! Runtime theme: semantic color roles and density. Panels name ROLES
//! (`text_muted`, `red_zone`), never values — the theme maps role to color,
//! so light mode and user themes become data changes, not code changes.

use crate::ui::tokens::{Density, font, radius, space};
use eframe::egui::{self, Color32};

#[derive(Debug, Clone)]
pub struct Theme {
    /// Whether the ground is light. Drives egui's own light/dark switch so
    /// stock widgets (text fields, scrollbars) follow the scheme instead of
    /// staying pinned to the dark defaults.
    pub light: bool,
    // grounds
    pub bg: Color32,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub surface_sunken: Color32,
    // content
    pub text: Color32,
    pub text_muted: Color32,
    /// Numeric readouts — pair with the monospace font.
    pub text_value: Color32,
    // lines
    pub outline: Color32,
    pub divider: Color32,
    pub focus: Color32,
    // identity
    pub accent: Color32,
    pub accent_muted: Color32,
    // --- parameter ROLE colours -------------------------------------
    //
    // Teenage Engineering's four-encoder palette, borrowed for what it
    // actually achieves: "colour is the natural link between on-screen
    // objects and encoders" (Sound on Sound on the OP-1). We have no
    // encoders, so the four hues name parameter FAMILIES instead —
    // which is the same trick aimed at the same problem, telling you
    // what kind of thing a control is before you read its name.
    //
    // Each has a DIM partner, because TE's active-parameter highlight
    // is not brightening the one you hold; it is dimming the three you
    // do not. Values are TE's own, from their guide's SVGs.
    /// Pitch and time — the blue encoder. Envelope stage curves.
    pub role_time: Color32,
    pub role_time_dim: Color32,
    /// Level and amount — the ochre encoder. Envelope peak and sustain.
    pub role_level: Color32,
    pub role_level_dim: Color32,
    /// Shape and choice — the off-white encoder. Waves, modes, colours.
    pub role_shape: Color32,
    pub role_shape_dim: Color32,
    /// Modulation and the destructive edge — the red encoder. Wires,
    /// drive, anything that rewrites another parameter.
    pub role_mod: Color32,
    pub role_mod_dim: Color32,
    // state
    pub ok: Color32,
    pub warn: Color32,
    pub danger: Color32,
    // engine vocabulary (the zones, promoted from lab hardcodes)
    pub red_zone: Color32,
    pub green_zone: Color32,
    // timeline
    pub playhead: Color32,
    pub loop_region: Color32,
    /// The loop brace itself: the region's own colour, at full strength.
    pub loop_brace: Color32,
    /// The time-selection wash, translucent so the grid reads through it.
    pub selection: Color32,
    pub grid_beat: Color32,
    pub grid_bar: Color32,
    /// Grid subdivisions, dimmer than a beat line.
    pub grid_sub: Color32,
    /// Alternating Arrangement lane grounds. These are authored separately
    /// from panel surfaces because the timeline must remain quieter than its
    /// headers while still exposing row boundaries without boxing them.
    pub timeline_lane: Color32,
    pub timeline_lane_alt: Color32,
    pub timeline_lane_selected: Color32,
    pub clip_body: Color32,
    /// Arrangement clips carry their track kind in their material rather
    /// than in a decorative icon: warm for MIDI, slate-blue for audio.
    pub clip_midi: Color32,
    pub clip_midi_header: Color32,
    pub clip_audio: Color32,
    pub clip_audio_header: Color32,
    pub clip_hover: Color32,
    pub clip_selected: Color32,
    /// Note bars inside clips — kept light enough to read on `clip_body`.
    pub clip_note: Color32,
    // --- the piano roll's notes -------------------------------------
    //
    // The note is the roll's content, so one channel carries one meaning
    // and no channel carries two: HUE is identity, CHROMA says whether
    // the note can sound at all, VALUE carries velocity inside a bounded
    // band, and the OUTLINE — nothing else — says selected.
    //
    // The consequence, and the reason these are authored as a pair rather
    // than picked independently: an UNSELECTED note must still read as
    // the same object as its selected neighbour. `clip_body` versus
    // `clip_selected` failed that badly — it swapped hue and lightness at
    // once, so the notes you were not holding looked disabled, which is
    // most notes, most of the time. `note_doctrine` in the tests holds
    // the line numerically.
    /// A note at full velocity, not selected. The roll's content colour.
    pub note_fill: Color32,
    /// The same note, selected: same hue, a bounded step in value and a
    /// small step in chroma. The RING does the shouting, not the fill.
    pub note_fill_selected: Color32,
    /// A hairline between adjacent notes, so a run of sixteenths reads as
    /// sixteen notes and not as one long bar.
    pub note_edge: Color32,
    /// Under the pointer: value moved, hue and chroma untouched.
    pub note_hover: Color32,
    /// Material from another clip or track, drawn for reference only —
    /// neutral hue, half chroma, never hit-tested.
    pub note_ghost: Color32,
    // audio
    pub meter_low: Color32,
    pub meter_hot: Color32,
    pub meter_clip: Color32,
    /// Multiplies spacing tokens: 1.0 comfortable, 0.85 compact.
    pub density: f32,
}

/// How close the pointer must get to a panel edge to drag it. Not a visual
/// value — nothing is drawn at this size — so it is not a spacing token.
const RESIZE_GRAB_PX: f32 = 8.0;

impl Theme {
    pub fn dark() -> Self {
        Self {
            // The ground ramp is charcoal rather than black. Its quiet warm
            // bias keeps the room from feeling clinical, while the larger
            // lightness steps make panel, control and well boundaries read
            // without needing outlines around everything.
            light: false,
            bg: Color32::from_rgb(0x15, 0x14, 0x12),
            surface: Color32::from_rgb(0x1d, 0x1b, 0x18),
            surface_raised: Color32::from_rgb(0x2a, 0x27, 0x22),
            surface_sunken: Color32::from_rgb(0x0d, 0x0c, 0x0b),
            text: Color32::from_rgb(0xe4, 0xe9, 0xed),
            text_muted: Color32::from_rgb(0x9b, 0xa8, 0xb1),
            text_value: Color32::from_rgb(0xc8, 0xda, 0xe4),
            // Lines keep the ground's tint but sit far enough above it to
            // separate adjacent cards and panel seams at a glance.
            outline: Color32::from_rgb(0x4a, 0x43, 0x3a),
            divider: Color32::from_rgb(0x35, 0x30, 0x2a),
            // A ring is BRIGHTER than what it surrounds: the accent's own hue,
            // lifted, so the scheme keeps its character and the mark that
            // says where the keyboard is stops hiding among emphasis.
            focus: Color32::from_rgb(0x8f, 0xdc, 0xf0),
            accent: Color32::from_rgb(0x5a, 0xb5, 0xd2),
            accent_muted: Color32::from_rgb(0x35, 0x62, 0x71),
            // TE's on-screen hues, bright and dim. Note their "white" is
            // a warm off-white and their "orange" is really a red — the
            // exact values matter, because the family is what makes the
            // four read as one system rather than four decorations.
            role_time: Color32::from_rgb(0x62, 0x91, 0xb9),
            role_time_dim: Color32::from_rgb(0x58, 0x6b, 0x77),
            role_level: Color32::from_rgb(0xc8, 0xa6, 0x7a),
            role_level_dim: Color32::from_rgb(0x55, 0x49, 0x3d),
            role_shape: Color32::from_rgb(0xdd, 0xdf, 0xd4),
            role_shape_dim: Color32::from_rgb(0x68, 0x70, 0x70),
            role_mod: Color32::from_rgb(0xf0, 0x54, 0x3d),
            role_mod_dim: Color32::from_rgb(0x52, 0x2b, 0x30),
            ok: Color32::from_rgb(0x6c, 0xc2, 0x72),
            warn: Color32::from_rgb(0xdc, 0xae, 0x67),
            danger: Color32::from_rgb(0xde, 0x70, 0x70),
            red_zone: Color32::from_rgb(0xf2, 0x7b, 0x68),
            green_zone: Color32::from_rgb(0x63, 0xc5, 0x96),
            playhead: Color32::from_rgb(0xed, 0xc9, 0x66),
            loop_region: Color32::from_rgba_unmultiplied(0x5a, 0xb5, 0xd2, 0x26),
            // The brace and its wash: the grid's own hue, brightened —
            // same hue (35deg) and saturation as `grid_bar`, three times the
            // lightness — so the loop reads as part of the grid rather than
            // as something imported from the panel seams.
            loop_brace: Color32::from_rgb(0xb1, 0x9e, 0x83),
            selection: Color32::from_rgba_premultiplied(0x4a, 0x64, 0x70, 0x42),
            grid_beat: Color32::from_rgb(0x32, 0x2e, 0x28),
            grid_bar: Color32::from_rgb(0x4a, 0x43, 0x39),
            grid_sub: Color32::from_rgb(0x25, 0x22, 0x1e),
            timeline_lane: Color32::from_rgb(0x15, 0x14, 0x12),
            timeline_lane_alt: Color32::from_rgb(0x18, 0x16, 0x13),
            timeline_lane_selected: Color32::from_rgb(0x20, 0x1f, 0x1c),
            // Clips live in the warm ground family like everything else: a
            // block one notch above `surface_raised`, a selection border in
            // the grid's own tan (the loop brace's hue, brightened), and
            // notes in the cream the bar uses for text.
            clip_body: Color32::from_rgb(0x3b, 0x35, 0x2d),
            clip_midi: Color32::from_rgb(0x4b, 0x40, 0x34),
            clip_midi_header: Color32::from_rgb(0x69, 0x58, 0x46),
            clip_audio: Color32::from_rgb(0x31, 0x47, 0x51),
            clip_audio_header: Color32::from_rgb(0x41, 0x63, 0x71),
            clip_hover: Color32::from_rgb(0x9b, 0xa8, 0xb1),
            clip_selected: Color32::from_rgb(0xd0, 0xb2, 0x8c),
            clip_note: Color32::from_rgb(0xee, 0xe5, 0xd9),
            // One hue (~34 deg, the grid's own tan) at four brightnesses.
            // Selected is 12% up in value and 5% up in chroma from the
            // plain note; hover is 8% up in value at identical chroma.
            note_fill: Color32::from_rgb(0xc9, 0xa0, 0x6b),
            note_fill_selected: Color32::from_rgb(0xe1, 0xb1, 0x73),
            note_edge: Color32::from_rgb(0x20, 0x1a, 0x15),
            note_hover: Color32::from_rgb(0xd9, 0xad, 0x74),
            note_ghost: Color32::from_rgb(0x6b, 0x62, 0x5a),
            meter_low: Color32::from_rgb(0x63, 0xc5, 0x96),
            meter_hot: Color32::from_rgb(0xdc, 0xae, 0x67),
            meter_clip: Color32::from_rgb(0xde, 0x70, 0x70),
            density: 1.0,
        }
    }

    /// The house light scheme: warm paper rather than bare white, with
    /// slate ink and the same restrained cyan identity as the dark side.
    ///
    /// This is authored role by role instead of mechanically inverting the
    /// dark theme. Wells remain recessed, raised surfaces catch light,
    /// timeline divisions recede in three distinct steps, and parameter
    /// families use darker pigments that remain readable on paper.
    pub fn light() -> Self {
        Self {
            light: true,
            bg: Color32::from_rgb(0xf3, 0xf0, 0xe9),
            surface: Color32::from_rgb(0xeb, 0xe7, 0xde),
            surface_raised: Color32::from_rgb(0xfc, 0xfa, 0xf5),
            surface_sunken: Color32::from_rgb(0xdd, 0xd8, 0xce),
            text: Color32::from_rgb(0x24, 0x28, 0x2d),
            text_muted: Color32::from_rgb(0x68, 0x74, 0x7d),
            text_value: Color32::from_rgb(0x34, 0x4c, 0x5b),
            outline: Color32::from_rgb(0xc4, 0xbe, 0xb2),
            divider: Color32::from_rgb(0xd8, 0xd2, 0xc7),
            // On a light ground the ring reads by going DARKER, which is the
            // same move as dark's — more present than the accent, not less.
            focus: Color32::from_rgb(0x0f, 0x5a, 0x78),
            accent: Color32::from_rgb(0x28, 0x7c, 0x9b),
            accent_muted: Color32::from_rgb(0xc6, 0xdd, 0xe5),
            role_time: Color32::from_rgb(0x37, 0x6f, 0x9d),
            role_time_dim: Color32::from_rgb(0xaa, 0xb9, 0xc3),
            role_level: Color32::from_rgb(0x9a, 0x6b, 0x2f),
            role_level_dim: Color32::from_rgb(0xcf, 0xc1, 0xae),
            role_shape: Color32::from_rgb(0x56, 0x5b, 0x52),
            role_shape_dim: Color32::from_rgb(0xba, 0xbd, 0xb5),
            role_mod: Color32::from_rgb(0xc5, 0x3f, 0x2f),
            role_mod_dim: Color32::from_rgb(0xd8, 0xb4, 0xae),
            ok: Color32::from_rgb(0x27, 0x7a, 0x50),
            warn: Color32::from_rgb(0xa5, 0x6c, 0x1b),
            danger: Color32::from_rgb(0xb5, 0x3d, 0x3d),
            red_zone: Color32::from_rgb(0xca, 0x44, 0x36),
            green_zone: Color32::from_rgb(0x26, 0x82, 0x5b),
            playhead: Color32::from_rgb(0xb3, 0x6c, 0x00),
            loop_region: Color32::from_rgba_unmultiplied(0x28, 0x7c, 0x9b, 0x22),
            loop_brace: Color32::from_rgb(0x5f, 0x78, 0x86),
            selection: Color32::from_rgba_unmultiplied(0x28, 0x7c, 0x9b, 0x30),
            grid_beat: Color32::from_rgb(0xdf, 0xd9, 0xce),
            grid_bar: Color32::from_rgb(0xc8, 0xc0, 0xb4),
            grid_sub: Color32::from_rgb(0xeb, 0xe6, 0xdd),
            timeline_lane: Color32::from_rgb(0xf3, 0xf0, 0xe9),
            timeline_lane_alt: Color32::from_rgb(0xee, 0xea, 0xe2),
            timeline_lane_selected: Color32::from_rgb(0xe4, 0xeb, 0xeb),
            clip_body: Color32::from_rgb(0xdd, 0xd4, 0xc5),
            clip_midi: Color32::from_rgb(0xdd, 0xd1, 0xbd),
            clip_midi_header: Color32::from_rgb(0xc7, 0xae, 0x89),
            clip_audio: Color32::from_rgb(0xc9, 0xdb, 0xe1),
            clip_audio_header: Color32::from_rgb(0x8d, 0xb8, 0xc6),
            clip_hover: Color32::from_rgb(0x68, 0x74, 0x7d),
            // Warm against a cool accent, which is dark's rule said on a light
            // ground — a selected clip and an emphasised control are
            // different claims and cannot be the same mark.
            clip_selected: Color32::from_rgb(0x8a, 0x6a, 0x3c),
            clip_note: Color32::from_rgb(0x59, 0x6d, 0x79),
            // The same hue on paper, and the step runs the other way:
            // on a light ground a selected note reads by going DARKER
            // and more saturated, not brighter. The doctrine is about
            // the size of the step, not its direction.
            note_fill: Color32::from_rgb(0x96, 0x68, 0x2c),
            note_fill_selected: Color32::from_rgb(0x88, 0x5a, 0x1e),
            note_edge: Color32::from_rgb(0x3a, 0x2a, 0x10),
            note_hover: Color32::from_rgb(0x8d, 0x62, 0x29),
            note_ghost: Color32::from_rgb(0xa9, 0x9e, 0x8e),
            meter_low: Color32::from_rgb(0x27, 0x7a, 0x50),
            meter_hot: Color32::from_rgb(0xa5, 0x6c, 0x1b),
            meter_clip: Color32::from_rgb(0xb5, 0x3d, 0x3d),
            density: 1.0,
        }
    }

    /// A dark industrial scheme: blue-black steel grounds, cool concrete
    /// text and restrained blue-green illumination. Saturation is reserved
    /// for interaction and musical material, so a busy project still reads
    /// like a tool rather than a wall of status lights.
    pub fn industrial() -> Self {
        Self {
            light: false,
            bg: Color32::from_rgb(0x0d, 0x12, 0x14),
            surface: Color32::from_rgb(0x15, 0x1c, 0x1f),
            surface_raised: Color32::from_rgb(0x20, 0x2a, 0x2e),
            surface_sunken: Color32::from_rgb(0x07, 0x0b, 0x0d),
            text: Color32::from_rgb(0xdc, 0xe8, 0xe8),
            text_muted: Color32::from_rgb(0x8d, 0xa0, 0xa2),
            text_value: Color32::from_rgb(0xa9, 0xd8, 0xd3),
            outline: Color32::from_rgb(0x3d, 0x51, 0x54),
            divider: Color32::from_rgb(0x29, 0x38, 0x3b),
            focus: Color32::from_rgb(0x9a, 0xf0, 0xe4),
            accent: Color32::from_rgb(0x42, 0xc7, 0xbb),
            accent_muted: Color32::from_rgb(0x23, 0x5e, 0x5d),
            role_time: Color32::from_rgb(0x65, 0xa9, 0xd8),
            role_time_dim: Color32::from_rgb(0x35, 0x55, 0x67),
            role_level: Color32::from_rgb(0x80, 0xc9, 0x9c),
            role_level_dim: Color32::from_rgb(0x36, 0x59, 0x49),
            role_shape: Color32::from_rgb(0xb8, 0xc8, 0xc6),
            role_shape_dim: Color32::from_rgb(0x5d, 0x70, 0x70),
            role_mod: Color32::from_rgb(0x8c, 0x9f, 0xdf),
            role_mod_dim: Color32::from_rgb(0x3e, 0x48, 0x69),
            ok: Color32::from_rgb(0x58, 0xbf, 0x8b),
            warn: Color32::from_rgb(0xd4, 0xa5, 0x5d),
            danger: Color32::from_rgb(0xd7, 0x68, 0x66),
            red_zone: Color32::from_rgb(0xe4, 0x77, 0x70),
            green_zone: Color32::from_rgb(0x69, 0xcb, 0x9e),
            playhead: Color32::from_rgb(0x73, 0xe3, 0xd5),
            loop_region: Color32::from_rgba_unmultiplied(0x42, 0xc7, 0xbb, 0x24),
            loop_brace: Color32::from_rgb(0x56, 0xb5, 0xae),
            selection: Color32::from_rgba_unmultiplied(0x62, 0xcf, 0xc4, 0x32),
            grid_beat: Color32::from_rgb(0x25, 0x34, 0x37),
            grid_bar: Color32::from_rgb(0x3c, 0x52, 0x55),
            grid_sub: Color32::from_rgb(0x19, 0x24, 0x27),
            timeline_lane: Color32::from_rgb(0x0e, 0x14, 0x16),
            timeline_lane_alt: Color32::from_rgb(0x11, 0x19, 0x1c),
            timeline_lane_selected: Color32::from_rgb(0x18, 0x29, 0x2b),
            clip_body: Color32::from_rgb(0x2c, 0x3b, 0x3e),
            clip_midi: Color32::from_rgb(0x25, 0x50, 0x4a),
            clip_midi_header: Color32::from_rgb(0x36, 0x72, 0x69),
            clip_audio: Color32::from_rgb(0x29, 0x46, 0x58),
            clip_audio_header: Color32::from_rgb(0x3b, 0x68, 0x80),
            clip_hover: Color32::from_rgb(0x83, 0xaf, 0xaf),
            clip_selected: Color32::from_rgb(0x85, 0xe0, 0xd5),
            clip_note: Color32::from_rgb(0xc2, 0xe5, 0xe0),
            note_fill: Color32::from_rgb(0x55, 0xb8, 0xac),
            note_fill_selected: Color32::from_rgb(0x60, 0xca, 0xbd),
            note_edge: Color32::from_rgb(0x0b, 0x20, 0x21),
            note_hover: Color32::from_rgb(0x5b, 0xc1, 0xb5),
            note_ghost: Color32::from_rgb(0x66, 0x70, 0x70),
            meter_low: Color32::from_rgb(0x4f, 0xb9, 0x88),
            meter_hot: Color32::from_rgb(0xd8, 0xad, 0x62),
            meter_clip: Color32::from_rgb(0xe0, 0x5e, 0x60),
            density: 1.0,
        }
    }

    /// A neon night scheme: violet-black grounds with cyan and magenta
    /// identity colours. The saturated accents stay on controls and musical
    /// content; panel grounds remain quiet enough for long sessions.
    pub fn cyberpunk() -> Self {
        Self {
            light: false,
            bg: Color32::from_rgb(0x09, 0x06, 0x11),
            surface: Color32::from_rgb(0x10, 0x0b, 0x1c),
            surface_raised: Color32::from_rgb(0x1c, 0x12, 0x30),
            surface_sunken: Color32::from_rgb(0x05, 0x03, 0x08),
            text: Color32::from_rgb(0xf2, 0xec, 0xff),
            text_muted: Color32::from_rgb(0xa8, 0x9a, 0xbc),
            text_value: Color32::from_rgb(0x58, 0xf7, 0xff),
            outline: Color32::from_rgb(0x63, 0x3d, 0x82),
            divider: Color32::from_rgb(0x2c, 0x1c, 0x43),
            // Neither the cyan a selected clip wears nor the magenta of the
            // accent: the ring has to be findable when both are on screen.
            focus: Color32::from_rgb(0xa8, 0xff, 0xe8),
            accent: Color32::from_rgb(0xff, 0x2b, 0xd6),
            accent_muted: Color32::from_rgb(0x55, 0x20, 0x4f),
            role_time: Color32::from_rgb(0x20, 0xe6, 0xff),
            role_time_dim: Color32::from_rgb(0x1d, 0x59, 0x70),
            // Amber, not the lemon a warning wears — a level's role colour
            // marks knobs and lanes that are always present, so it cannot
            // be the colour that means something is wrong.
            role_level: Color32::from_rgb(0xff, 0xc2, 0x66),
            role_level_dim: Color32::from_rgb(0x65, 0x59, 0x24),
            role_shape: Color32::from_rgb(0xe8, 0xdc, 0xff),
            role_shape_dim: Color32::from_rgb(0x68, 0x5e, 0x7b),
            role_mod: Color32::from_rgb(0xff, 0x38, 0xa8),
            role_mod_dim: Color32::from_rgb(0x68, 0x20, 0x50),
            ok: Color32::from_rgb(0x52, 0xff, 0x91),
            warn: Color32::from_rgb(0xff, 0xe6, 0x3d),
            danger: Color32::from_rgb(0xff, 0x42, 0x68),
            red_zone: Color32::from_rgb(0xff, 0x42, 0x68),
            green_zone: Color32::from_rgb(0x52, 0xff, 0x91),
            // The brightest thing that moves, and its own colour: a warning
            // must be able to appear against it.
            playhead: Color32::from_rgb(0xff, 0xf5, 0x9a),
            loop_region: Color32::from_rgba_unmultiplied(0xff, 0x2b, 0xd6, 0x28),
            loop_brace: Color32::from_rgb(0xff, 0x62, 0xe2),
            selection: Color32::from_rgba_unmultiplied(0x00, 0xf5, 0xff, 0x30),
            grid_beat: Color32::from_rgb(0x25, 0x18, 0x38),
            grid_bar: Color32::from_rgb(0x4c, 0x2c, 0x66),
            grid_sub: Color32::from_rgb(0x18, 0x0f, 0x25),
            timeline_lane: Color32::from_rgb(0x09, 0x06, 0x11),
            timeline_lane_alt: Color32::from_rgb(0x0d, 0x08, 0x18),
            timeline_lane_selected: Color32::from_rgb(0x1a, 0x0d, 0x29),
            clip_body: Color32::from_rgb(0x29, 0x17, 0x3c),
            clip_midi: Color32::from_rgb(0x52, 0x19, 0x58),
            clip_midi_header: Color32::from_rgb(0x9f, 0x27, 0x91),
            clip_audio: Color32::from_rgb(0x0e, 0x4b, 0x61),
            clip_audio_header: Color32::from_rgb(0x12, 0x89, 0xa1),
            clip_hover: Color32::from_rgb(0xc4, 0x70, 0xe8),
            clip_selected: Color32::from_rgb(0x00, 0xf5, 0xff),
            clip_note: Color32::from_rgb(0xb7, 0xfa, 0xff),
            note_fill: Color32::from_rgb(0x23, 0xdb, 0xe4),
            note_fill_selected: Color32::from_rgb(0x2a, 0xf3, 0xfd),
            note_edge: Color32::from_rgb(0x06, 0x15, 0x19),
            note_hover: Color32::from_rgb(0x26, 0xe8, 0xf1),
            note_ghost: Color32::from_rgb(0x72, 0x6c, 0x78),
            meter_low: Color32::from_rgb(0x52, 0xff, 0x91),
            meter_hot: Color32::from_rgb(0xff, 0xe6, 0x3d),
            meter_clip: Color32::from_rgb(0xff, 0x42, 0x68),
            density: 1.0,
        }
    }

    /// The same theme at a different packing. Density is a machine-local
    /// preference, so it arrives from `ui::prefs`, not from a project.
    pub fn with_density(mut self, density: Density) -> Self {
        self.density = density.scale();
        self
    }

    pub fn set_density(&mut self, density: Density) {
        self.density = density.scale();
    }

    /// Density-scaled spacing: `theme.sp(space::MD)`.
    pub fn sp(&self, token: f32) -> f32 {
        token * self.density
    }

    /// Project the theme into egui so BUILT-IN widgets comply too. Without
    /// this, every stock widget is a leak in the token system.
    pub fn apply(&self, ctx: &egui::Context) {
        // egui 0.36 keeps one Style per light/dark theme; we are the theme
        // system, so pin egui to whichever side the ground is on and shape
        // that one style.
        ctx.set_theme(if self.light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        });
        ctx.all_styles_mut(|style| self.shape_style(style));
    }

    fn shape_style(&self, style: &mut egui::Style) {
        // egui's default side-resize hit zone is 3px, which makes a panel
        // edge something you aim at rather than something you grab. Widen it:
        // the handle is invisible either way, so its size is pure ergonomics.
        style.interaction.resize_grab_radius_side = RESIZE_GRAB_PX;

        style.spacing.item_spacing = egui::vec2(self.sp(space::SM), self.sp(space::XS));
        style.spacing.button_padding = egui::vec2(self.sp(space::SM), self.sp(space::XS));
        style.spacing.menu_margin = egui::Margin::same(self.sp(space::SM) as i8);
        style.spacing.window_margin = egui::Margin::same(self.sp(space::MD) as i8);

        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(font::LABEL, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(font::BODY, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(font::BODY, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(font::TITLE, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(font::VALUE, FontFamily::Monospace),
            ),
        ]
        .into();

        let v = &mut style.visuals;
        v.dark_mode = !self.light;
        v.panel_fill = self.surface;
        v.window_fill = self.surface;
        v.extreme_bg_color = self.surface_sunken;
        v.faint_bg_color = self.surface_raised;
        v.override_text_color = Some(self.text);
        v.hyperlink_color = self.accent;
        v.selection.bg_fill = self.accent_muted;
        v.selection.stroke = egui::Stroke::new(crate::ui::tokens::stroke::HAIR, self.accent);
        v.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(crate::ui::tokens::stroke::HAIR, self.divider);
        v.widgets.inactive.bg_fill = self.surface_raised;
        v.widgets.hovered.bg_fill = self.surface_raised;
        v.widgets.active.bg_fill = self.accent_muted;
        v.widgets.inactive.corner_radius = egui::CornerRadius::same(radius::CTRL as u8);
        v.widgets.hovered.corner_radius = egui::CornerRadius::same(radius::CTRL as u8);
        v.widgets.active.corner_radius = egui::CornerRadius::same(radius::CTRL as u8);
        v.window_corner_radius = egui::CornerRadius::same(radius::PANEL as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hue in degrees, chroma and value in `0..=1`. Hand-rolled because
    /// the doctrine is stated in these three channels and pulling a crate
    /// in to check six colours would be worse than nine lines of maths.
    fn hsv(c: Color32) -> (f32, f32, f32) {
        let (r, g, b) = (
            f32::from(c.r()) / 255.0,
            f32::from(c.g()) / 255.0,
            f32::from(c.b()) / 255.0,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let range = max - min;
        let hue = if range < 1e-6 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / range).rem_euclid(6.0))
        } else if max == g {
            60.0 * ((b - r) / range + 2.0)
        } else {
            60.0 * ((r - g) / range + 4.0)
        };
        let sat = if max <= 0.0 { 0.0 } else { range / max };
        (hue, sat, max)
    }

    /// The colour doctrine, held numerically so it cannot be re-litigated
    /// by a well-meaning eyedropper. See the `note_*` field comments.
    #[test]
    fn unselected_notes_are_not_greyed() {
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::industrial(),
            Theme::cyberpunk(),
        ] {
            let (h_plain, s_plain, v_plain) = hsv(theme.note_fill);
            let (h_sel, s_sel, v_sel) = hsv(theme.note_fill_selected);
            let (h_hover, s_hover, v_hover) = hsv(theme.note_hover);

            // Hue is identity: selection and hover may not move it.
            assert!(
                (h_plain - h_sel).abs() < 4.0,
                "selection shifted the hue: {h_plain} vs {h_sel}"
            );
            assert!(
                (h_plain - h_hover).abs() < 4.0,
                "hover shifted the hue: {h_plain} vs {h_hover}"
            );

            // Value: the step is bounded in BOTH directions, because a
            // selected note that is twice as bright is a different object,
            // and one that is identical is not selected.
            let v_ratio = v_plain / v_sel;
            assert!(
                (0.88..=1.14).contains(&v_ratio),
                "unselected/selected value ratio {v_ratio} outside the band"
            );
            assert!(
                (v_plain - v_sel).abs() > 0.02,
                "selection made no visible difference at all"
            );

            // Chroma: an unselected note is never washed out relative to
            // a selected one. This is the actual greyed-out bug.
            assert!(
                s_plain / s_sel >= 0.85,
                "unselected chroma {s_plain} is under 85% of selected {s_sel}"
            );

            // Hover moves value only.
            assert!(
                (s_plain - s_hover).abs() < 0.03,
                "hover changed the chroma: {s_plain} vs {s_hover}"
            );
            assert!((v_plain - v_hover).abs() > 0.015, "hover is invisible");

            // A ghost is the one note colour allowed to be neutral, and
            // it must be unmistakably neutral rather than merely dimmer.
            let (_, s_ghost, _) = hsv(theme.note_ghost);
            assert!(
                s_ghost < s_plain * 0.5,
                "a ghost at chroma {s_ghost} is not distinct from live material"
            );

            // The hairline separates notes, so it must not BE the note.
            let (_, _, v_edge) = hsv(theme.note_edge);
            assert!(
                (v_edge - v_plain).abs() > 0.2,
                "the note edge does not separate anything"
            );
        }
    }

    /// The roll's notes are the content of the panel they sit in, so they
    /// must out-contrast the ground they sit on — in both schemes.
    #[test]
    fn notes_read_against_the_grid_ground() {
        fn luminance(c: Color32) -> f32 {
            fn chan(v: u8) -> f32 {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            }
            0.2126 * chan(c.r()) + 0.7152 * chan(c.g()) + 0.0722 * chan(c.b())
        }
        fn contrast(a: Color32, b: Color32) -> f32 {
            let (x, y) = (luminance(a), luminance(b));
            let (hi, lo) = if x > y { (x, y) } else { (y, x) };
            (hi + 0.05) / (lo + 0.05)
        }
        for theme in [
            Theme::dark(),
            Theme::light(),
            Theme::industrial(),
            Theme::cyberpunk(),
        ] {
            // Against both row grounds: white keys and the recessed
            // black-key rows. A note must be obvious on either.
            assert!(
                contrast(theme.note_fill, theme.surface) >= 3.0,
                "a plain note does not read on the grid"
            );
            assert!(
                contrast(theme.note_fill, theme.surface_sunken) >= 3.0,
                "a plain note does not read on a black-key row"
            );
            // And the softest note the velocity band can produce still does.
            let floor = theme.note_fill.gamma_multiply(0.72);
            assert!(
                contrast(floor, theme.surface) >= 2.0,
                "a velocity-1 note vanishes into the grid"
            );
        }
    }
}
