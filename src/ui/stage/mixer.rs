//! The mixer — the lattice's other content.
//!
//! The session already draws tracks ACROSS as columns, and a mixer makes
//! the same claim about the same objects: one column, one track. So this
//! is not a second surface with a second geometry to reconcile — it is
//! what hangs beneath the heads while the scenes are put away. The heads
//! never move across the change, which is what lets the eye keep its
//! place.
//!
//! # One ruler
//!
//! A meter and a fader are two different quantities, and drawing them on
//! one axis invites reading them as one. They are therefore put on the
//! SAME ruler rather than two: decibels, from [`meter::FLOOR_DB`] to
//! unity at the top, with the small boost the model allows drawn as
//! overtravel ABOVE the meter's ceiling. Unity is exactly the line the
//! meter tops out at. A fader at unity and a meter reading full scale
//! then mean the same height, because they are the same number.
//!
//! The alternative — the fader on its own normalised span — puts unity at
//! two thirds of the way up while full scale is at the top, and the eye
//! reads the two marks as disagreeing about what "the top" is.
//!
//! # Why nothing is dimmed to show focus
//!
//! Mixing is COMPARISON: the level of one track against another. A
//! surface that dims every channel but the focused one destroys exactly
//! the reading it exists to support. So every meter is drawn at the same
//! strength, and the cursor is carried by the frame around a channel — a
//! mark of its own, in the value focus always uses, spent on the one
//! thing that is focus rather than on the biggest thing on screen.
//!
//! # What the blocks are
//!
//! The meter is a stack of cells lit from the bottom, and the topmost lit
//! cell is a DITHER — a hard-coded pattern of sub-squares at ordered
//! coverage, which is what buys intermediate steps without spending new
//! symbols from the alphabet: one ink, more or less of it. Vectors and
//! not characters, for the reason `ui::glyph` gives: a character carries
//! a typeface's opinion, sits on a baseline that is not the cell's
//! centre, and becomes a box on a machine without the font.

use super::Level;
use crate::design;
use crate::sequencing::Song;
use crate::ui::device::meter;
use eframe::egui;

/// Cells in one meter. Enough that the eye reads a bar rather than
/// counting, few enough that a single cell is a coarse, findable unit —
/// about four decibels at this span.
pub const CELLS: usize = 14;

/// The most the model lets a fader add: `track.volume` is registered over
/// `0.0..=1.5` linear, and 1.5 is this many decibels. Declared here as
/// what it IS rather than as a round number, so the drawn overtravel
/// cannot drift from the range the document actually permits.
pub const BOOST_DB: f32 = 3.521_825_2;

/// One track reduced to exactly what a channel strip draws.
///
/// Holds no state of its own and is rebuilt every frame from the song and
/// the last thing the engine said — a strip that remembered anything
/// could show a fader the document does not have.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Channel {
    /// Fader, as linear amplitude. 1.0 is unity.
    pub gain: f32,
    /// Constant-power pan, `-1..=1`, centre exact.
    pub pan: f32,
    /// Its own switch, regardless of what that means for what is heard.
    pub muted: bool,
    pub soloed: bool,
    /// Whether this track ACTUALLY sounds — which is neither of the two
    /// above. A track silenced by someone else's solo is not muted, and a
    /// mixer that could not draw that third state would be lying about
    /// two of them.
    pub audible: bool,
    /// Whether this channel has switches to draw at all.
    ///
    /// The master has none: everything that sounds arrives there, so
    /// there is nothing above it to silence it and nothing beside it to
    /// solo against. Two squares that cannot be pressed would be two
    /// controls promising something the surface will not do.
    pub switches: bool,
    /// The last level the engine reported. Silence where there is no
    /// engine, which is the truth about a stage with nothing behind it.
    pub level: Level,
}

/// Every track as a channel, in the song's own order.
pub fn channels(song: &Song, levels: &[Level]) -> Vec<Channel> {
    song.tracks
        .iter()
        .enumerate()
        .map(|(index, track)| Channel {
            gain: track.volume,
            pan: track.pan,
            muted: track.muted,
            soloed: track.solo,
            audible: song.audible(index),
            switches: true,
            level: levels.get(index).copied().unwrap_or_default(),
        })
        .collect()
}

/// Where a decibel sits in a strip, as `0..=1` from the bottom.
///
/// The span runs from the meter's floor to unity PLUS the boost the model
/// allows, so one mapping serves the meter and the fader both.
pub fn place_of_db(db: f32) -> f32 {
    let span = BOOST_DB - meter::FLOOR_DB;
    if db.is_nan() {
        return 0.0;
    }
    ((db - meter::FLOOR_DB) / span).clamp(0.0, 1.0)
}

/// Unity's place on that ruler: the line the meter tops out at, and where
/// a fader that neither adds nor takes away sits.
pub fn unity_place() -> f32 {
    place_of_db(0.0)
}

/// A linear amplitude's place on the ruler. Silence is the floor rather
/// than minus infinity, because a mark has to be somewhere.
pub fn place_of_amp(amp: f32) -> f32 {
    let db = meter::amp_to_db(amp);
    if db.is_infinite() {
        0.0
    } else {
        place_of_db(db)
    }
}

/// How much of cell `index` is lit by a level standing at `place`.
///
/// Cells divide the METER's span — floor to unity — rather than the whole
/// ruler: the boost band above unity is the fader's overtravel and has no
/// meter behind it, because a level above full scale is clipping and not
/// a taller bar.
pub fn cell_coverage(index: usize, place: f32) -> f32 {
    let ceiling = unity_place();
    if ceiling <= 0.0 {
        return 0.0;
    }
    let per_cell = ceiling / CELLS as f32;
    let floor = index as f32 * per_cell;
    ((place - floor) / per_cell).clamp(0.0, 1.0)
}

/// The ordered-dither matrix the pseudo-blocks are built from.
///
/// A 4×4 Bayer threshold map: sixteen coverage steps from one ink, which
/// is the whole trick — an intermediate value is AREA, not a new colour,
/// so the alphabet's cap is untouched by however many steps are drawn.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Paint one pseudo-block: `rect` filled to `coverage` in `ink`.
///
/// Full coverage is one rectangle rather than sixteen, which is both
/// faster and sharper; anything between is the dither. Sub-squares are
/// snapped to whole pixels, or the pattern moirés at fractional scaling
/// and the meter shimmers while standing still.
pub fn block(painter: &egui::Painter, rect: egui::Rect, ink: egui::Color32, coverage: f32) {
    let coverage = coverage.clamp(0.0, 1.0);
    if coverage <= 0.0 {
        return;
    }
    if coverage >= 1.0 {
        painter.rect_filled(snap(painter, rect), 0.0, ink);
        return;
    }
    let steps = (coverage * 16.0).round() as u8;
    let cell = egui::vec2(rect.width() / 4.0, rect.height() / 4.0);
    for (row, thresholds) in BAYER.iter().enumerate() {
        for (col, threshold) in thresholds.iter().enumerate() {
            if *threshold >= steps {
                continue;
            }
            let min = rect.min + egui::vec2(col as f32 * cell.x, row as f32 * cell.y);
            painter.rect_filled(
                snap(painter, egui::Rect::from_min_size(min, cell)),
                0.0,
                ink,
            );
        }
    }
}

/// Pull a rectangle onto the pixel grid. A dither is a pattern of
/// hairlines; half a pixel of drift is the difference between a texture
/// and a shimmer.
fn snap(painter: &egui::Painter, rect: egui::Rect) -> egui::Rect {
    let ppp = painter.ctx().pixels_per_point();
    if ppp <= 0.0 {
        return rect;
    }
    let to = |v: f32| (v * ppp).round() / ppp;
    egui::Rect::from_min_max(
        egui::pos2(to(rect.min.x), to(rect.min.y)),
        egui::pos2(to(rect.max.x), to(rect.max.y)),
    )
}

/// The most the model lets a fader hold, as linear amplitude: the top of
/// `track.volume`'s registered span. Named here so the verb and the drawn
/// overtravel cannot come to disagree about the same number.
pub const MAX_GAIN: f32 = 1.5;

/// One press of the fader, in decibels. The unit the ear works in, so a
/// press is the same SIZE of change everywhere on the rail rather than a
/// huge one at the top and an inaudible one at the bottom.
pub const GAIN_STEP_DB: f32 = 1.0;

/// A held modifier's press: a tenth of one, for setting rather than
/// finding.
pub const GAIN_FINE_DB: f32 = 0.1;

/// One press of the pan, over a `-1..=1` span: twenty presses corner to
/// corner, ten from the centre to either side.
pub const PAN_STEP: f32 = 0.1;

/// Move a fader by `db`, and hand back what the track's volume becomes.
///
/// Two ends, both of which have to be reachable BY HAND rather than by
/// luck: the top is the model's own limit, and the bottom is true
/// silence — not the meter's floor, which is merely the quietest thing
/// the meter can draw. A fader that could only approach silence would
/// leave the one value a musician reaches for most out of reach.
pub fn step_gain(volume: f32, db: f32) -> f32 {
    let from = if volume <= 0.0 {
        meter::FLOOR_DB
    } else {
        meter::amp_to_db(volume)
    };
    let to = from + db;
    if to <= meter::FLOOR_DB {
        // The floor and everything under it is silence, EXACTLY. The
        // floor itself is included so the gesture is its own inverse: a
        // press up from silence leaves the floor by one decibel, and one
        // press back down returns to silence rather than landing on a
        // level too quiet for the meter to draw but not quiet enough to
        // be off.
        return 0.0;
    }
    meter::db_to_amp(to.min(BOOST_DB)).clamp(0.0, MAX_GAIN)
}

/// Move a pan by `delta`, and hand back what the track's pan becomes.
///
/// Centre is EXACT in the model, so a step that would cross it LANDS on
/// it instead. Without that, a pan stepped by tenths from an odd place
/// steps over the centre forever and "back to the middle" becomes a
/// thing only a reset can do.
pub fn step_pan(pan: f32, delta: f32) -> f32 {
    let to = pan + delta;
    if pan != 0.0 && (pan < 0.0) != (to < 0.0) {
        return 0.0;
    }
    to.clamp(-1.0, 1.0)
}

/// A fader's value, said out loud. Decibels, because that is what it was
/// edited in — and silence says so in words rather than as a number no
/// scale contains.
pub fn gain_label(volume: f32) -> String {
    if volume <= 0.0 {
        return "-inf dB".to_owned();
    }
    let db = meter::amp_to_db(volume);
    // Anything that would print as zero IS zero. Without this a fader
    // walked back to unity reads "-0.0 dB", which puts a sign on a value
    // that has none — and the sign is the first thing the eye takes off
    // this readout.
    let db = if db.abs() < 0.05 { 0.0 } else { db };
    format!("{db:+.1} dB")
}

/// A pan's value: which way, and how far, counted in whole percent so
/// the number is short enough to read at a glance. Centre is a WORD,
/// because centre is a state and not a small number.
pub fn pan_label(pan: f32) -> String {
    let amount = (pan.abs() * 100.0).round() as i32;
    if amount == 0 {
        return "centre".to_owned();
    }
    let side = if pan < 0.0 { 'L' } else { 'R' };
    format!("{side}{amount}")
}

/// The pan rail's height at the foot of a strip. A layout dimension,
/// settled by eye: one rail and its mark, and no more.
pub const PAN_H: f32 = 12.0;

/// The two switches' row. Square by construction — a switch is a square
/// because a square at this size is a shape rather than a glyph, and it
/// survives the distance the meter is meant to be read from.
pub const SWITCH_H: f32 = 16.0;

/// Where a channel strip hangs: everything from beneath the head down to
/// the foot of the field. The column IS the track's address, so the strip
/// takes exactly the head's width and never computes one of its own.
pub fn strip_beneath(head: egui::Rect, bottom: f32, gap: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(head.min.x, head.max.y + gap),
        egui::pos2(head.max.x, bottom),
    )
}

/// Draw one channel.
///
/// Everything here is symmetric about the strip's centre line — the two
/// meters, the handle across them, the switch pair, the pan rail's centre
/// tick — because the thing being read at a distance is a DEPARTURE from
/// centre, and a symmetric ground is what makes a departure visible.
pub fn draw(
    painter: &egui::Painter,
    strip: egui::Rect,
    channel: &Channel,
    gap: f32,
    alpha: &design::Alphabet,
) {
    if strip.height() <= 0.0 || strip.width() <= 0.0 {
        return;
    }
    let pan_top = strip.max.y - PAN_H;
    let switch_top = pan_top - gap - SWITCH_H;
    let meters = egui::Rect::from_min_max(strip.min, egui::pos2(strip.max.x, switch_top - gap));
    if meters.height() > 0.0 {
        draw_meters(painter, meters, channel, gap, alpha);
    }
    if channel.switches {
        draw_switches(
            painter,
            egui::Rect::from_min_max(
                egui::pos2(strip.min.x, switch_top),
                egui::pos2(strip.max.x, switch_top + SWITCH_H),
            ),
            channel,
            gap,
            alpha,
        );
    }
    draw_pan(
        painter,
        egui::Rect::from_min_max(egui::pos2(strip.min.x, pan_top), strip.max),
        channel,
        alpha,
    );
}

/// The two meters and the fader that crosses them.
fn draw_meters(
    painter: &egui::Painter,
    zone: egui::Rect,
    channel: &Channel,
    gap: f32,
    alpha: &design::Alphabet,
) {
    // The ink a level is drawn in. LIVE is the alphabet's word for the
    // sounding present — meters in motion are its named use. A track that
    // CANNOT sound is drawn in structure ink instead: it has a meter, and
    // the meter has nothing to say.
    let ink = if channel.audible {
        alpha.live.color
    } else {
        alpha.edge.color
    };
    let rest = alpha.well.color;

    // Cells fill the ladder from the floor to unity; the band above it is
    // the fader's overtravel, which has no meter behind it.
    let unity_y = zone.max.y - zone.height() * unity_place();
    let ladder = egui::Rect::from_min_max(egui::pos2(zone.min.x, unity_y), zone.max);
    let cell_h = ladder.height() / CELLS as f32;

    let gutter = gap * 0.5;
    let meter_w = ((zone.width() - gutter) / 2.0).max(1.0);
    let sides = [
        (zone.min.x, channel.level.left),
        (zone.max.x - meter_w, channel.level.right),
    ];
    for (x, amp) in sides {
        let place = place_of_amp(amp);
        // Over full scale there is nothing taller to draw, so the top
        // cell changes its VALUE instead of its height — the one place
        // this surface spends alarm ink, and it spends it on the only
        // thing here that is actually at stake.
        let clipping = amp >= 1.0;
        for cell in 0..CELLS {
            let top = ladder.max.y - (cell + 1) as f32 * cell_h;
            let rect = egui::Rect::from_min_size(
                egui::pos2(x, top + cell_h * 0.15),
                egui::vec2(meter_w, cell_h * 0.7),
            );
            let coverage = cell_coverage(cell, place);
            if coverage <= 0.0 {
                // The unlit ladder stays visible: a meter reading nothing
                // and a meter that is not there must not look alike.
                block(painter, rect, rest, 1.0);
                continue;
            }
            let lit = if clipping && cell + 1 == CELLS {
                alpha.jeopardy_active.color
            } else {
                ink
            };
            block(painter, rect, lit, coverage);
        }
    }

    // The scale. Graduations at the decibels a mixer is actually read
    // against, drawn up the strip's leading edge. This is most of what
    // separates an instrument from a drawing of one: a value you can
    // read off a rail without a number beside it.
    let marks: Vec<f32> = [0.0f32, -6.0, -12.0, -24.0, -48.0]
        .into_iter()
        .map(place_of_db)
        .collect();
    super::ornament::graduations(painter, zone, &marks, alpha.edge.color, gap * 0.6);

    // Unity, as a rule across the whole strip: the line the meters top
    // out at and the fader's own zero — one line, because it is one number.
    painter.line_segment(
        [
            egui::pos2(zone.min.x, unity_y),
            egui::pos2(zone.max.x, unity_y),
        ],
        egui::Stroke::new(1.0, alpha.edge.color),
    );

    // The handle: the gain, on the same ruler, across both meters. Drawn
    // last so it is never hidden by a loud meter — the fader is a thing
    // the hand set and must stay findable while the music moves.
    let handle_y = zone.max.y - zone.height() * place_of_amp(channel.gain);
    painter.line_segment(
        [
            egui::pos2(zone.min.x, handle_y),
            egui::pos2(zone.max.x, handle_y),
        ],
        egui::Stroke::new(3.0, alpha.ink.color),
    );
}

/// Mute and solo, as two squares. POSITION is the code — left is mute,
/// right is solo — because a letter is unreadable at the distance this
/// surface is meant to be read from, and a square is not.
fn draw_switches(
    painter: &egui::Painter,
    zone: egui::Rect,
    channel: &Channel,
    gap: f32,
    alpha: &design::Alphabet,
) {
    let size = SWITCH_H.min(zone.height()).min(zone.width());
    let centre = zone.center().x;
    let switches = [
        (centre - gap * 0.5 - size, channel.muted),
        (centre + gap * 0.5, channel.soloed),
    ];
    for (x, on) in switches {
        let rect = egui::Rect::from_min_size(egui::pos2(x, zone.min.y), egui::vec2(size, size));
        if on {
            block(painter, rect, alpha.ink.color, 1.0);
        } else {
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, alpha.edge.color),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// The pan rail: centre is exact in the model, so it is exact here — a
/// tick at the middle, and the mark that departs from it.
fn draw_pan(
    painter: &egui::Painter,
    zone: egui::Rect,
    channel: &Channel,
    alpha: &design::Alphabet,
) {
    let mid = zone.center();
    painter.line_segment(
        [egui::pos2(zone.min.x, mid.y), egui::pos2(zone.max.x, mid.y)],
        egui::Stroke::new(1.0, alpha.edge.color),
    );
    let half = zone.width() / 2.0;
    let x = mid.x + channel.pan.clamp(-1.0, 1.0) * half;
    let mark =
        egui::Rect::from_center_size(egui::pos2(x, mid.y), egui::vec2(3.0, zone.height() * 0.6));
    block(painter, mark, alpha.ink.color, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequencing::TrackKind;

    #[test]
    fn a_fader_reaches_silence_and_the_models_own_ceiling() {
        // Down from silence stays silent; the bottom is a floor, not a hole.
        assert_eq!(step_gain(0.0, -GAIN_STEP_DB), 0.0);

        // And up from silence leaves it, so the gesture is reversible.
        let up = step_gain(0.0, GAIN_STEP_DB);
        assert!(up > 0.0, "a silenced fader could not be brought back");
        assert_eq!(
            step_gain(up, -GAIN_STEP_DB),
            0.0,
            "one press down from one press up was not silence again"
        );

        // The top is the document's limit and nothing beyond it.
        let mut loud = 1.0;
        for _ in 0..20 {
            loud = step_gain(loud, GAIN_STEP_DB);
        }
        assert!(
            loud <= MAX_GAIN,
            "the fader went past what the document allows ({loud})"
        );
        assert!(
            (loud - MAX_GAIN).abs() < 1e-3,
            "the fader stopped short ({loud})"
        );
    }

    #[test]
    fn a_fader_step_is_the_same_size_wherever_it_is_pressed() {
        for start in [0.05f32, 0.25, 0.5, 1.0] {
            let moved = step_gain(start, GAIN_STEP_DB);
            let grew = meter::amp_to_db(moved) - meter::amp_to_db(start);
            assert!(
                (grew - GAIN_STEP_DB).abs() < 1e-3,
                "a press at {start} moved {grew} dB, not {GAIN_STEP_DB}"
            );
        }
    }

    #[test]
    fn unity_is_reachable_by_hand_from_either_side() {
        // A press is a whole decibel and unity is zero of them, so it is
        // an exact place on the rail rather than one to be approached.
        let below = step_gain(meter::db_to_amp(-1.0), GAIN_STEP_DB);
        assert!(
            (below - 1.0).abs() < 1e-4,
            "coming up, unity was missed ({below})"
        );
        let above = step_gain(meter::db_to_amp(1.0), -GAIN_STEP_DB);
        assert!(
            (above - 1.0).abs() < 1e-4,
            "coming down, unity was missed ({above})"
        );
    }

    #[test]
    fn a_pan_crossing_the_centre_lands_on_it() {
        // The case that makes this necessary: an odd starting place.
        let odd = 0.05;
        assert_eq!(
            step_pan(odd, -PAN_STEP),
            0.0,
            "the pan stepped over the centre"
        );
        assert_eq!(step_pan(-0.05, PAN_STEP), 0.0);
        // And having landed there, the next press leaves.
        assert!((step_pan(0.0, -PAN_STEP) + PAN_STEP).abs() < 1e-6);
        // The ends hold.
        assert_eq!(step_pan(1.0, PAN_STEP), 1.0);
        assert_eq!(step_pan(-1.0, -PAN_STEP), -1.0);
    }

    #[test]
    fn the_values_say_themselves() {
        assert_eq!(gain_label(0.0), "-inf dB");
        assert_eq!(gain_label(1.0), "+0.0 dB");
        // Walked back to unity rather than set to it: the arithmetic
        // leaves a hair either side, and neither may grow a sign.
        assert_eq!(
            gain_label(step_gain(meter::db_to_amp(-1.0), GAIN_STEP_DB)),
            "+0.0 dB"
        );
        assert_eq!(
            gain_label(step_gain(meter::db_to_amp(1.0), -GAIN_STEP_DB)),
            "+0.0 dB"
        );
        assert_eq!(gain_label(0.5), "-6.0 dB");
        assert_eq!(pan_label(0.0), "centre");
        assert_eq!(pan_label(-0.5), "L50");
        assert_eq!(pan_label(1.0), "R100");
    }

    #[test]
    fn a_channel_reports_the_tracks_own_mixer_facts() {
        let mut song = Song::default();
        song.tracks[0].volume = 0.5;
        song.tracks[0].pan = -0.25;
        let channels = channels(&song, &[]);
        assert_eq!(channels[0].gain, 0.5);
        assert_eq!(channels[0].pan, -0.25);
        assert!(channels[0].audible);
        assert_eq!(
            channels[0].level,
            Level::default(),
            "a meter with no engine behind it invented a reading"
        );
    }

    #[test]
    fn the_third_state_is_visible_as_itself() {
        let mut song = Song::default();
        song.add_track(TrackKind::Instrument);
        song.tracks[1].solo = true;
        let channels = channels(&song, &[]);
        assert!(
            !channels[0].muted && !channels[0].audible,
            "a track silenced by another's solo read as muted or as sounding"
        );
        assert!(channels[1].audible && channels[1].soloed);
    }

    #[test]
    fn unity_is_the_line_the_meter_tops_out_at() {
        // Full scale and a fader at unity are the same number, so they
        // must be the same height. That is the whole point of one ruler.
        assert_eq!(place_of_amp(1.0), unity_place());
        assert!(
            unity_place() < 1.0,
            "no room left for the boost the model allows"
        );
        assert!(unity_place() > 0.9, "the boost band swallowed the meter");
    }

    #[test]
    fn the_ruler_runs_from_the_floor_to_the_boost() {
        assert_eq!(place_of_db(meter::FLOOR_DB), 0.0);
        assert_eq!(place_of_db(BOOST_DB), 1.0);
        assert_eq!(
            place_of_db(meter::FLOOR_DB - 20.0),
            0.0,
            "below the floor is the floor"
        );
        assert_eq!(place_of_amp(0.0), 0.0, "silence has to be somewhere");
    }

    #[test]
    fn the_boost_band_is_exactly_what_the_model_permits() {
        // 1.5 linear is the registered top of `track.volume`.
        let top = meter::amp_to_db(1.5);
        assert!(
            (top - BOOST_DB).abs() < 1e-4,
            "the drawn overtravel ({BOOST_DB}) is not the range the document allows ({top})"
        );
    }

    #[test]
    fn silence_lights_nothing_and_full_scale_lights_everything() {
        let silent = place_of_amp(0.0);
        let full = place_of_amp(1.0);
        for cell in 0..CELLS {
            assert_eq!(
                cell_coverage(cell, silent),
                0.0,
                "cell {cell} lit at silence"
            );
            // Full scale is the top of the ladder by construction, so the
            // last cell only misses it by the arithmetic — not by a rung.
            assert!(
                cell_coverage(cell, full) > 0.999,
                "cell {cell} dark at full scale"
            );
        }
    }

    #[test]
    fn cells_light_from_the_bottom_and_the_top_one_is_partial() {
        // Deliberately NOT half: half of the ladder falls exactly on a
        // cell boundary, which is the one level with no partial cell.
        let level = unity_place() * 0.55;
        let lit: Vec<f32> = (0..CELLS).map(|cell| cell_coverage(cell, level)).collect();
        assert_eq!(lit[0], 1.0, "the bottom cell is not full below half scale");
        assert_eq!(lit[CELLS - 1], 0.0, "the top cell lit below half scale");
        // Never brighter going up: a meter that did would not read as a bar.
        for pair in lit.windows(2) {
            assert!(pair[0] >= pair[1], "coverage climbed: {lit:?}");
        }
        assert!(
            lit.iter().any(|c| *c > 0.0 && *c < 1.0),
            "no partial cell — the meter can only move a whole cell at a time"
        );
    }

    #[test]
    fn a_level_on_a_cell_boundary_is_the_one_with_no_partial_cell() {
        // Stated as a test rather than left as a surprise: exactly seven
        // fourteenths lights seven whole cells and no fraction.
        let lit: Vec<f32> = (0..CELLS)
            .map(|cell| cell_coverage(cell, unity_place() / 2.0))
            .collect();
        assert!(lit.iter().all(|c| *c == 0.0 || *c == 1.0), "{lit:?}");
    }
}
