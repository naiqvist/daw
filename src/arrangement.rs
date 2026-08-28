//! The arrangement: lanes, clips, the ruler, automation and the minimap.
//!
//! The largest region `main.rs` was carrying, and the one two agents
//! kept colliding in. It comes across in one piece because its
//! coordinate story is already pure — `beat_at` and `x_at` are
//! inverses at any offset, and every hit test and every paint goes
//! through them, so there is no second idea of where a beat is to
//! reconcile.
//!
//! The clip helpers travel with it. They read as model rather than
//! drawing, but every one of them exists to answer a question the
//! drawing asks — where a dragged edge may land, which gap a dropped
//! clip fits, whether a lane will take it.
use super::*;

/// Beats <-> pixels, and snapping. All pure; the arrangement's whole
/// coordinate story lives in these four functions.
///
/// `offset` is the beat shown at the area's left edge — where the view has
/// been panned to. Beats are absolute musical time; the offset is only
/// where the window sits. Both functions are inverses at any offset.
pub(crate) fn beat_at(area: egui::Rect, offset: f32, pixels_per_beat: f32, x: f32) -> f32 {
    (offset + (x - area.left()) / pixels_per_beat.max(ARRANGEMENT_ZOOM_MIN)).max(0.0)
}

pub(crate) fn x_at(area: egui::Rect, offset: f32, pixels_per_beat: f32, beat: f32) -> f32 {
    area.left() + (beat - offset) * pixels_per_beat
}

pub(crate) fn snap(beat: f32, grid: f32) -> f32 {
    if grid <= 0.0 {
        return beat.max(0.0);
    }
    ((beat / grid).round() * grid).max(0.0)
}

/// Order a drag's two ends and guarantee it spans at least one grid unit.
///
/// A click with no drag would otherwise select zero time, and Ctrl+L on zero
/// time can only no-op — which looks like a broken shortcut rather than an
/// empty selection.
pub(crate) fn span(a: f32, b: f32, grid: f32) -> (f32, f32) {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    if hi - lo < grid {
        (lo, lo + grid)
    } else {
        (lo, hi)
    }
}

/// The grid cell at `beat` in `lane`: one grid division wide, the lane's
/// full height.
pub(crate) fn cell_rect(
    content: egui::Rect,
    offset: f32,
    pixels_per_beat: f32,
    lane: egui::Rect,
    beat: f32,
    grid: f32,
) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(x_at(content, offset, pixels_per_beat, beat), lane.top()),
        egui::pos2(
            x_at(content, offset, pixels_per_beat, beat + grid),
            lane.bottom(),
        ),
    )
}

/// The selection covering every cell between `anchor` and `cursor`.
///
/// Inclusive at both ends: anchoring on beat 4 and extending to beat 6
/// selects cells 4, 5 and 6, so the range runs to `6 + grid`. With the two
/// equal it is one cell, which is exactly what plain movement produces.
///
/// Pure, so the extend arithmetic is checkable in both directions.
pub(crate) fn extended(anchor: f32, cursor: f32, grid: f32) -> (f32, f32) {
    (anchor.min(cursor), anchor.max(cursor) + grid)
}

/// Step the grid one rung, clamped at both ends.
///
/// Clamped rather than wrapped: walking off the fine end and reappearing at
/// 1/1 would be a nasty surprise mid-edit. Pure, so the ladder is testable.
pub(crate) fn step_grid(grid: usize, finer: bool) -> usize {
    if finer {
        (grid + 1).min(GRID_BEATS.len() - 1)
    } else {
        grid.saturating_sub(1)
    }
}

// --- clips: geometry and overlap clamps, all pure -------------------------

/// A clip's rect in its lane. The caller clips it to the visible area.
pub(crate) fn clip_rect(
    content: egui::Rect,
    offset: f32,
    pixels_per_beat: f32,
    lane: egui::Rect,
    clip: &Clip,
) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            x_at(content, offset, pixels_per_beat, clip.start),
            lane.top(),
        ),
        egui::pos2(
            x_at(content, offset, pixels_per_beat, clip.start + clip.len),
            lane.bottom(),
        ),
    )
}

/// The visual zones inside a timeline clip. `clip_rect` remains the exact
/// musical extent; this helper adds only presentation insets, so snapping,
/// overlap and edge arithmetic never inherit decorative padding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClipCanvasRects {
    pub(crate) outer: egui::Rect,
    pub(crate) title: egui::Rect,
    pub(crate) content: egui::Rect,
    pub(crate) left_grip: egui::Rect,
    pub(crate) right_grip: egui::Rect,
}

pub(crate) fn clip_canvas_rects(full: egui::Rect) -> ClipCanvasRects {
    let pad_y = CLIP_LANE_PAD_Y.min((full.height() - 1.0).max(0.0) * 0.5);
    let outer = egui::Rect::from_min_max(
        egui::pos2(full.left(), full.top() + pad_y),
        egui::pos2(full.right(), full.bottom() - pad_y),
    );
    let title_h = CLIP_TITLE_H.min(outer.height());
    let title =
        egui::Rect::from_min_max(outer.min, egui::pos2(outer.right(), outer.top() + title_h));
    let content = egui::Rect::from_min_max(
        egui::pos2(outer.left(), title.bottom()),
        outer.right_bottom(),
    );
    let grip_h = CLIP_GRIP_H.min(outer.height());
    let grip_y = outer.center().y - grip_h * 0.5;
    let left_grip = egui::Rect::from_min_size(
        egui::pos2(outer.left(), grip_y),
        egui::vec2(CLIP_GRIP_W.min(outer.width()), grip_h),
    );
    let right_grip = egui::Rect::from_min_size(
        egui::pos2((outer.right() - CLIP_GRIP_W).max(outer.left()), grip_y),
        egui::vec2(CLIP_GRIP_W.min(outer.width()), grip_h),
    );
    ClipCanvasRects {
        outer,
        title,
        content,
        left_grip,
        right_grip,
    }
}

/// Where a note's bar goes inside a clip's rect, given the clip's pitch
/// range. Higher pitches sit higher in the block, the way a piano roll
/// reads, and every bar is at least `NOTE_MIN_H` tall so quiet notes do not
/// vanish into the fill.
pub(crate) fn note_rect(
    area: egui::Rect,
    note: &Note,
    pitch_lo: u8,
    pitch_hi: u8,
    clip_len: f32,
) -> egui::Rect {
    let span = (pitch_hi - pitch_lo + 1) as f32;
    let frac = (note.pitch - pitch_lo) as f32 / span;
    let band = area.height() / span;
    let y = area.top() + (1.0 - frac) * area.height() - band * 0.5;
    // Notes carry musical time in f64; the lane's geometry is f32.
    let x = area.left() + (note.start as f32 / clip_len) * area.width();
    let w = ((note.len as f32 / clip_len) * area.width()).max(1.0);
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, band.max(NOTE_MIN_H)))
}

/// The bounds a clip's start must respect: at or after the previous clip's
/// end, at or before the next clip's start minus its own length, never
/// negative. `clips` is sorted by start — it always is, by construction.
///
/// When neighbours leave no room at all, the answer is "stay put": a
/// pinned clip is better than one that jumped through a neighbour.
pub(crate) fn clamp_clip_start(clips: &[Clip], idx: usize, want: f32) -> f32 {
    let len = clips[idx].len;
    let lo = idx
        .checked_sub(1)
        .map(|p| clips[p].start + clips[p].len)
        .unwrap_or(0.0);
    let hi = clips
        .get(idx + 1)
        .map(|n| n.start - len)
        .unwrap_or(f32::INFINITY);
    if hi < lo {
        return clips[idx].start;
    }
    want.clamp(lo, hi).max(0.0)
}

/// The bounds a clip's length must respect while its start stays put: one
/// grid unit minimum, the gap to the next clip maximum.
/// The LENGTH a right-edge drag is asking for, from the beat under the
/// pointer.
///
/// Trivial arithmetic, and it was wrong for months: the caller passed
/// the pointer's absolute beat straight into a parameter that means
/// length, so a clip grew by exactly its own start every time an edge
/// was dragged. A clip at bar three pulled to bar four became seven
/// bars long.
///
/// It survived because a clip at beat zero gets the right answer by
/// coincidence — and a clip at beat zero is what almost every fixture
/// is. Pulled out here so the one line can be held by a test that puts
/// the clip somewhere else.
pub(crate) fn drag_len(want: f32, start: f32, grid: f32) -> f32 {
    // The END is snapped, not the length: a clip that begins off the
    // grid should still be draggable onto a grid line, and snapping the
    // length instead would carry its offset into every edge it ever has.
    snap(want, grid) - start
}

pub(crate) fn clamp_clip_len(clips: &[Clip], idx: usize, want: f32, grid: f32) -> f32 {
    let hi = clips
        .get(idx + 1)
        .map(|n| n.start - clips[idx].start)
        .unwrap_or(f32::INFINITY);
    if hi < grid {
        return clips[idx].len;
    }
    want.clamp(grid, hi)
}

pub(crate) fn scripted_clip_len(clip: &Clip, edit: piano_roll::ClipLengthEdit, grid: f32) -> f32 {
    let fit = || {
        clip.notes
            .iter()
            .map(|note| (note.start + note.len) as f32)
            .fold(grid, f32::max)
    };
    match edit {
        piano_roll::ClipLengthEdit::Set(length) => length,
        piano_roll::ClipLengthEdit::Extend(length) => clip.len + length,
        piano_roll::ClipLengthEdit::Trim(length) => clip.len - length,
        piano_roll::ClipLengthEdit::Fit => fit(),
    }
    .max(grid)
}

pub(crate) fn set_scripted_clip_len(clip: &mut Clip, length: f32) {
    clip.len = length;
    if clip.loop_on {
        clip.loop_start = clip.loop_start.min(length);
        clip.loop_len = clip.loop_len.min((length - clip.loop_start).max(0.0));
        if clip.loop_len <= 0.0 {
            clip.loop_on = false;
        }
    }
}

/// Apply a non-destructive left trim. Moving the arrangement edge right
/// advances the source region by the same real-time duration; moving it back
/// reveals frames up to file frame zero. The source END stays invariant, so
/// a later expansion can restore audio that an earlier trim hid.
pub(crate) fn trim_clip_left(clip: &mut Clip, start: f32, bpm: f64) {
    let old_start = clip.start;
    let end = clip.start + clip.len;
    clip.start = start;
    clip.len = end - start;
    if let Some(audio) = &mut clip.audio {
        let delta_beats = f64::from(start - old_start);
        let delta_frames =
            (delta_beats * 60.0 / bpm.max(1.0) * f64::from(audio.sample_rate)).round() as i128;
        let source_end = audio.source_offset.saturating_add(audio.source_frames);
        let offset = (i128::from(audio.source_offset) + delta_frames)
            .clamp(0, i128::from(source_end)) as u64;
        audio.source_offset = offset;
        audio.source_frames = source_end - offset;
    }
}

/// Keep a track's clips sorted by start after an edit. Order is what the
/// clamp functions assume — this is the function that restores it.
pub(crate) fn resort(track: &mut [Clip]) {
    track.sort_by(|a, b| a.start.total_cmp(&b.start));
}

/// Where a NEW clip of `len` beats can live: the first gap at or after `at`
/// that fits it, else the end of the track. Returns (start, insertion
/// index). The final gap is infinite, so there is always an answer — new
/// clips never fail to place, they just land further right than asked.
///
/// Pure; the placement policy behind create, paste and duplicate alike.
pub(crate) fn place_clip(track: &[Clip], at: f32, len: f32) -> (f32, usize) {
    let idx = track.partition_point(|c| c.start < at);
    for i in idx..=track.len() {
        let lo = i
            .checked_sub(1)
            .map(|p| track[p].start + track[p].len)
            .unwrap_or(0.0);
        let hi = track.get(i).map(|n| n.start).unwrap_or(f32::INFINITY);
        if hi - lo >= len {
            return (at.clamp(lo, hi - len).max(0.0), i);
        }
    }
    let end = track.last().map(|c| c.start + c.len).unwrap_or(0.0);
    (end, track.len())
}

/// Where each lane sits, top to bottom, and the boundary below it.
///
/// Pure, so lane stacking and the resize maths are checkable without a
/// window. Lanes past the bottom of the view are still returned — the caller
/// decides what to draw.
/// Does this track reach the mixer?
///
/// Solo-in-place: while ANYTHING is soloed, only soloed tracks sound. Mute
/// wins over solo, so muting a soloed track still silences it — which is
/// what both buttons being lit has to mean.
///
/// The one answer, used twice: `build_graph_spec` decides what to wire from
/// it, and the header dims the tracks it says are silent. A header that
/// disagreed with the schedule would be the worst possible bug here.
/// Everything about the tracks that changes the graph's SHAPE, hashed.
///
/// Loading or removing a device adds or drops a node; so does muting a
/// track, soloing one (which drops every other), or a track's KIND, since
/// an audio track compiles to no sequencer at all. None of that can be
/// expressed as a parameter letter, so a change here must swap the schedule
/// immediately rather than wait for the clip debounce.
///
/// A hash rather than the bitmask this used to be: there is no longer a
/// fixed number of bits per track, and the mask silently stopped covering
/// tracks past the 32nd.
///
/// PAN IS NOT IN HERE, deliberately: every instrument track always carries
/// a Pan node, so pan rides a letter and a knob drag costs nothing.
pub(crate) fn shape_hash(tracks: &[Track], master: &MasterTrack, returns: &[ReturnTrack]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // A return is a bus of nodes, and its MUTE removes the bus entirely
    // along with every send that fed it — so both are shape. Its level
    // and pan are not, for the reason a lane's are not: they ride the
    // output stage every return always has.
    //
    // A SEND LEVEL IS NOT SHAPE EITHER, and that is the whole point of
    // compiling a gain node for every pair: opening a send from silence
    // is a letter, so a drag from zero costs no recompile.
    // A route and its monitor are SHAPE: they add or drop input nodes,
    // which no parameter letter can do. The monitor especially — it is
    // the difference between a node in the schedule and no node at all,
    // and it must take effect the moment it is switched.
    for track in tracks {
        std::mem::discriminant(&track.input).hash(&mut hasher);
        match track.input {
            TrackInput::None => {}
            TrackInput::Mono(channel) => channel.hash(&mut hasher),
            TrackInput::Stereo(left, right) => {
                left.hash(&mut hasher);
                right.hash(&mut hasher);
            }
        }
        track.monitor.hash(&mut hasher);
        // The ARM is shape under `Monitor::Auto`, where it is the whole
        // of what decides whether an input node exists.
        track.armed.hash(&mut hasher);
    }
    returns.len().hash(&mut hasher);
    for bus in returns {
        bus.mute.hash(&mut hasher);
        bus.chain.len().hash(&mut hasher);
        for instance in &bus.chain {
            instance.id.hash(&mut hasher);
            std::mem::discriminant(&instance.state).hash(&mut hasher);
            instance.bypass.hash(&mut hasher);
            instance.parent.hash(&mut hasher);
        }
    }
    // The master's chain is shape for exactly the reasons a lane's is: its
    // devices are nodes, and no letter can add one. Its LEVEL is not —
    // that rides `sync_master`, like every other fader.
    master.chain.len().hash(&mut hasher);
    for instance in &master.chain {
        instance.id.hash(&mut hasher);
        instance.kind().hash(&mut hasher);
        instance.bypass.hash(&mut hasher);
        if let DeviceState::Echo(params) = instance.state {
            (params.send > 0.0).hash(&mut hasher);
        }
    }
    for t in tracks {
        t.kind.hash(&mut hasher);
        t.mute.hash(&mut hasher);
        t.solo.hash(&mut hasher);
        // Nesting is WIRING: which bus a lane lands on, and whether it
        // is a bus at all. Neither can travel as a parameter letter.
        t.is_group.hash(&mut hasher);
        t.depth.hash(&mut hasher);
        // The chain by IDENTITY and order: adding, removing, reordering or
        // bypassing a device all change which nodes exist and how they are
        // wired, and none of that can travel as a parameter letter.
        t.chain.len().hash(&mut hasher);
        for instance in &t.chain {
            instance.id.hash(&mut hasher);
            instance.kind().hash(&mut hasher);
            instance.bypass.hash(&mut hasher);
            // A delay's SEND is a shape the moment it leaves zero: an
            // insert stands in the signal path, an aux hangs off the
            // output stage, and no letter can move a node from one to
            // the other. The LEVEL is not in here — only which side of
            // zero it is on — so riding an established send costs a
            // letter, exactly like riding a fader.
            if let DeviceState::Echo(params) = instance.state {
                (params.send > 0.0).hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

/// Which lanes have something to record, and from where.
///
/// A free function over the lanes, so the rule can be read and tested
/// without an app around it — it is the one decision that turns a
/// pressed record button into files.
///
/// An armed lane with no input route is skipped rather than given a file
/// of silence: being armed says what you INTEND, and the route is what
/// makes it possible. A group is skipped because its sound is the lanes
/// under it, which are recording themselves.
pub(crate) fn record_routes(tracks: &[Track]) -> Vec<record::RecordRoute> {
    tracks
        .iter()
        .enumerate()
        .filter(|(_, track)| track.armed && track.kind == TrackKind::Audio && !track.is_group)
        .filter_map(|(index, track)| {
            let channels = match track.input {
                TrackInput::None => return None,
                TrackInput::Mono(channel) => vec![channel],
                TrackInput::Stereo(left, right) => vec![left, right],
            };
            Some(record::RecordRoute {
                track: index,
                channels,
            })
        })
        .collect()
}

pub(crate) fn track_audible(tracks: &[Track], i: usize) -> bool {
    if i >= tracks.len() {
        return false;
    }
    // Mute and solo both read through the GROUPS above the lane: a
    // group's switch is the whole point of a group, one control that
    // takes the drums out however many lanes the drums are.
    if track::muted_in_place(tracks, i) {
        return false;
    }
    let any_solo = tracks.iter().any(|t| t.solo);
    !any_solo || track::solo_in_scope(tracks, i)
}

/// Whether a lane's kind can hold a clip: audio clips ride audio tracks,
/// note clips ride instrument tracks. What a cross-lane drag checks before
/// letting the ghost change lanes.
pub(crate) fn lane_accepts(track: &Track, clip: &Clip) -> bool {
    if clip.audio.is_some() {
        track.kind == TrackKind::Audio
    } else {
        track.kind.takes_instrument()
    }
}

/// What a wheel gesture asks the timeline for: beats SIDEWAYS, points
/// DOWN.
///
/// Pure, because the two signs are the whole thing that can be wrong
/// here and neither is visible in a screenshot until you are already
/// scrolling the wrong way.
///
/// egui's convention is a scroll area's: a positive delta means the
/// CONTENT moves down, so the offset into it goes down too — hence the
/// negation on the vertical. The horizontal keeps the sign the timeline
/// has always panned by, so Shift+wheel now does exactly what a bare
/// wheel did before this change.
pub(crate) fn wheel_axes(scroll: egui::Vec2, pixels_per_beat: f32) -> (f32, f32) {
    let per_beat = if pixels_per_beat.is_finite() && pixels_per_beat > 0.0 {
        pixels_per_beat
    } else {
        return (0.0, 0.0);
    };
    (scroll.x / per_beat, -scroll.y)
}

/// Which lane a point falls in, or the nearest one when it falls past
/// the ends.
///
/// NEAREST rather than `None`, because this answers "which track is the
/// marquee reaching?" — and a drag that runs off the bottom of the last
/// lane plainly means that lane, not "no selection". A folded lane has
/// zero height and can never be the answer, which is correct: it is not
/// on screen to be dragged over.
pub(crate) fn lane_at(lanes: &[egui::Rect], y: f32) -> Option<usize> {
    let mut nearest: Option<(usize, f32)> = None;
    for (index, lane) in lanes.iter().enumerate() {
        if lane.height() <= 0.0 {
            continue;
        }
        if (lane.top()..lane.bottom()).contains(&y) {
            return Some(index);
        }
        let gap = if y < lane.top() {
            lane.top() - y
        } else {
            y - lane.bottom()
        };
        if nearest.is_none_or(|(_, best)| gap < best) {
            nearest = Some((index, gap));
        }
    }
    nearest.map(|(index, _)| index)
}

/// Every clip inside the band: tracks `lanes.0..=lanes.1`, and any part
/// of the clip inside `from..to`.
///
/// TOUCHING counts, not containment. Live selects a clip the marquee
/// merely crosses, and the alternative is a band that has to be drawn
/// exactly around a clip to take it — which on a timeline you have
/// scrolled and zoomed is a gesture nobody lands.
///
/// Pure, so the whole rule is testable without a window.
pub(crate) fn clips_in_band(
    clips: &[Vec<Clip>],
    lanes: (usize, usize),
    from: f32,
    to: f32,
) -> Vec<(usize, usize)> {
    let (first, last) = (lanes.0.min(lanes.1), lanes.0.max(lanes.1));
    let (start, end) = (from.min(to), from.max(to));
    let mut hits = Vec::new();
    for track in first..=last {
        let Some(lane) = clips.get(track) else {
            continue;
        };
        for (index, clip) in lane.iter().enumerate() {
            // A zero-width band still takes what it lands on: dragging
            // out a selection and coming back to the start is not the
            // same gesture as never having pressed.
            let touches = clip.start < end || (start == end && clip.start <= end);
            if touches && clip.start + clip.len > start {
                hits.push((track, index));
            }
        }
    }
    hits
}

pub(crate) fn lane_rects(area: egui::Rect, tracks: &[Track], scroll_y: f32) -> Vec<egui::Rect> {
    let mut y = area.top() - scroll_y;
    tracks
        .iter()
        .enumerate()
        .map(|(index, t)| {
            // A lane inside a folded group gets a rect of ZERO HEIGHT
            // where the next visible lane begins — not `Rect::NOTHING`.
            // Every reader of this list either skips an empty lane or
            // asks whether the pointer is inside it, and both answer
            // correctly for an empty rect; a rect at infinity would make
            // the "past the bottom, stop drawing" test fire on the first
            // hidden lane and take every lane after it down too.
            let height = if track::hidden_by_fold(tracks, index) {
                0.0
            } else {
                t.height
            };
            let rect = egui::Rect::from_min_max(
                egui::pos2(area.left(), y),
                egui::pos2(area.right(), y + height),
            );
            y += height;
            rect
        })
        .collect()
}

/// Give the canvas a quiet row rhythm before the timing grid is painted.
/// Selection changes the ground by one authored step; it does not erase the
/// grid or flood the lane with the global focus colour.
pub(crate) fn paint_lane_bands(
    ui: &egui::Ui,
    content: egui::Rect,
    lanes: &[egui::Rect],
    selected: Option<usize>,
    theme: &Theme,
) {
    let painter = ui.painter();
    painter.rect_filled(content, 0.0, theme.timeline_lane);
    for (i, lane) in lanes.iter().enumerate() {
        let visible = lane.intersect(content);
        if visible.height() <= 0.0 {
            continue;
        }
        let fill = if selected == Some(i) {
            theme.timeline_lane_selected
        } else if i % 2 == 0 {
            theme.timeline_lane
        } else {
            theme.timeline_lane_alt
        };
        painter.rect_filled(visible, 0.0, fill);
    }
}

/// The compact Arrangement ruler: bars are numbered, beats are ticks, and
/// subdivisions stay in the canvas. This preserves hierarchy at any zoom
/// instead of turning the ruler into a duplicate of the full-height grid.
pub(crate) fn arrangement_ruler(
    ui: &egui::Ui,
    ruler: egui::Rect,
    content: egui::Rect,
    theme: &Theme,
    arr: &Arrangement,
    beats_per_bar: u32,
    pixels_per_beat: f32,
) {
    let painter = ui.painter();
    painter.rect_filled(ruler, 0.0, theme.surface_sunken);
    painter.line_segment(
        [ruler.left_bottom(), ruler.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    let per_bar = beats_per_bar.max(1) as f32;
    let mut beat = arr.view_beats.floor();
    while x_at(content, arr.view_beats, pixels_per_beat, beat) <= ruler.right() {
        let x = x_at(content, arr.view_beats, pixels_per_beat, beat);
        if x >= ruler.left() {
            let on_bar = (beat % per_bar).abs() < 1e-3;
            let tick_h = if on_bar { ruler.height() } else { space::XS };
            painter.line_segment(
                [
                    egui::pos2(x, ruler.bottom() - tick_h),
                    egui::pos2(x, ruler.bottom()),
                ],
                egui::Stroke::new(
                    stroke::HAIR,
                    if on_bar {
                        theme.grid_bar
                    } else {
                        theme.grid_beat
                    },
                ),
            );
            if on_bar && pixels_per_beat * per_bar >= 36.0 {
                let bar = (beat / per_bar).floor() as i64 + 1;
                painter.text(
                    egui::pos2(x + space::XS, ruler.top() + 1.0),
                    egui::Align2::LEFT_TOP,
                    bar.to_string(),
                    egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                    theme.text_muted,
                );
            }
        }
        beat += 1.0;
    }
}

/// Paint the beat grid across `area`.
///
/// Three weights: bars, beats, and whatever subdivision the grid is set to.
/// Subdivisions are dropped entirely when they would land closer together
/// than `GRID_MIN_PX` — at 1/32 and this zoom that is 3px apart, which reads
/// as a wash rather than a grid.
/// `pixels_per_beat` is explicit rather than read from `arr`: the focused
/// automation editor draws at a different scale than the timeline, and the
/// grid must be at the same scale as the thing snapping to it.
pub(crate) fn beat_grid(
    ui: &egui::Ui,
    area: egui::Rect,
    theme: &Theme,
    arr: &Arrangement,
    beats_per_bar: u32,
    pixels_per_beat: f32,
) {
    let painter = ui.painter();
    let sub = arr.grid_beats();
    let per_bar = beats_per_bar.max(1) as f32;
    let step = grid_step(sub, per_bar, pixels_per_beat);

    // Start at the first line at or before the left edge, so lines land on
    // absolute beat/bar boundaries no matter where the view is panned.
    let mut beat = (arr.view_beats / step).floor() * step;
    loop {
        let x = x_at(area, arr.view_beats, pixels_per_beat, beat);
        if x > area.right() {
            break;
        }
        let on_bar = (beat % per_bar).abs() < 1e-3;
        let on_beat = (beat.fract()).abs() < 1e-3;
        let colour = if on_bar {
            theme.grid_bar
        } else if on_beat {
            theme.grid_beat
        } else {
            theme.grid_sub
        };
        painter.line_segment(
            [egui::pos2(x, area.top()), egui::pos2(x, area.bottom())],
            egui::Stroke::new(1.0, colour),
        );
        beat += step;
    }
}

// Automation is a real sublane, never an overlay on the clips. Keeping its
// geometry in one place means the lane body, clip pass, and automation pass
// agree about which pixels belong to which interaction.
pub(crate) const AUTOMATION_LANE_H: f32 = 34.0;

pub(crate) fn automation_rect(lane: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            lane.left(),
            (lane.bottom() - AUTOMATION_LANE_H).max(lane.top()),
        ),
        lane.right_bottom(),
    )
}

/// The compact automation sublane shown below clips on the selected track.
/// It deliberately edits one parameter at a time: touch a familiar target,
/// then draw, instead of making every lane a wall of unrelated curves.
// This is the rendering/input boundary for one envelope; its parameters are
// deliberately explicit so the compact lane and focused editor share it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn automation_lane(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    theme: &Theme,
    view_beats: f32,
    pixels_per_beat: f32,
    grid: f32,
    target: &str,
    spec: &ParameterSpec,
    automation: &mut TrackAutomation,
    base: f32,
) -> bool {
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, theme.surface_sunken.gamma_multiply(0.8));
    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );
    let lo = spec.min;
    let hi = spec.max;
    let point_pos = |beat: f32, value: f32| {
        let x = x_at(rect, view_beats, pixels_per_beat, beat);
        let y = rect.bottom() - ((value - lo) / (hi - lo)).clamp(0.0, 1.0) * rect.height();
        egui::pos2(x, y)
    };
    let value_at_y = |y: f32| {
        let value = (lo + (rect.bottom() - y) / rect.height() * (hi - lo)).clamp(lo, hi);
        if spec.stepped { value.round() } else { value }
    };
    let start = view_beats;
    let end = start + rect.width() / pixels_per_beat;
    // Sample the evaluated curve rather than joining dots with straight
    // strokes. This keeps the drawn line exactly faithful to playback for
    // every segment shape, including a stepped hold.
    let samples = (rect.width() / 6.0).ceil().clamp(2.0, 512.0) as usize;
    let curve: Vec<_> = (0..=samples)
        .map(|i| {
            let beat = start + (end - start) * i as f32 / samples as f32;
            point_pos(beat, automation.value_at(target, beat, base))
        })
        .collect();
    painter.add(egui::Shape::line(
        curve,
        egui::Stroke::new(1.5, theme.accent),
    ));

    let id = ui.id().with(("automation_lane", target));
    // The handle is deliberately invisible. The line itself is the target:
    // when it is under the pointer it advertises a vertical curve gesture;
    // holding Alt/Option turns that gesture into a bend edit for its source
    // point, matching Live's envelope editing convention.
    let hit_curve = |pointer: egui::Pos2| {
        if !rect.contains(pointer) {
            return None;
        }
        let mut best: Option<(usize, f32)> = None;
        for (index, pair) in automation.points(target).windows(2).enumerate() {
            let [a, b] = pair else { continue };
            if pointer.x
                < point_pos(a.beat, a.value)
                    .x
                    .min(point_pos(b.beat, b.value).x)
                    - 6.0
                || pointer.x
                    > point_pos(a.beat, a.value)
                        .x
                        .max(point_pos(b.beat, b.value).x)
                        + 6.0
            {
                continue;
            }
            let mut previous = point_pos(a.beat, a.value);
            for step in 1..=24 {
                let beat = a.beat + (b.beat - a.beat) * step as f32 / 24.0;
                let next = point_pos(beat, automation.value_at(target, beat, base));
                let segment = next - previous;
                let length_sq = segment.length_sq().max(f32::EPSILON);
                let t = ((pointer - previous).dot(segment) / length_sq).clamp(0.0, 1.0);
                let distance_sq = (pointer - (previous + segment * t)).length_sq();
                if distance_sq <= 49.0 && best.is_none_or(|(_, closest)| distance_sq < closest) {
                    best = Some((index, distance_sq));
                }
                previous = next;
            }
        }
        best.map(|(index, _)| index)
    };
    let curve_hit = ui.ctx().pointer_latest_pos().and_then(hit_curve);
    let curve_press_hit = ui
        .input(|input| input.pointer.press_origin())
        .and_then(hit_curve);
    let curve_modifier_held = ui.input(|i| i.modifiers.alt);
    if let Some(index) = curve_hit {
        let points = automation.points(target);
        if let Some([a, b]) = points.get(index..=index + 1) {
            let highlighted: Vec<_> = (0..=32)
                .map(|step| {
                    let beat = a.beat + (b.beat - a.beat) * step as f32 / 32.0;
                    point_pos(beat, automation.value_at(target, beat, base))
                })
                .collect();
            painter.add(egui::Shape::line(
                highlighted,
                egui::Stroke::new(3.0, theme.text),
            ));
        }
    }
    let response = ui
        .interact(rect, id, egui::Sense::click_and_drag())
        .affords(Affords::Draw);
    let mut hovered = response.hovered();
    if hovered {
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.5, theme.accent),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.right_top() + egui::vec2(-5.0, 4.0),
            egui::Align2::RIGHT_TOP,
            "Z  focus automation",
            egui::FontId::proportional(9.0),
            theme.text,
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    if response.clicked()
        && !(curve_modifier_held && curve_hit.is_some())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let freehand = ui.input(|i| i.modifiers.alt);
        let raw_beat = beat_at(rect, view_beats, pixels_per_beat, pos.x).max(0.0);
        let beat = if freehand {
            raw_beat
        } else {
            snap(raw_beat, grid)
        };
        let value = value_at_y(pos.y);
        automation.insert(target, beat, value);
    }
    if curve_hit.is_some() {
        hovered = true;
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    // Keep the segment and its starting bend from the press onward. The
    // primary lane response owns the pointer for the complete drag, so this
    // cannot be lost to a second invisible interaction layer mid-gesture.
    let curve_drag_id = id.with("curve_drag");
    if response.drag_started() {
        // Armed (or explicitly DISARMED) on every press: a stale segment
        // from the last gesture must never answer a drag that started on
        // empty lane space.
        match curve_press_hit.filter(|_| curve_modifier_held) {
            Some(index) => {
                let bend = automation.points(target)[index].bend;
                ui.ctx()
                    .data_mut(|data| data.insert_temp(curve_drag_id, (index, bend)));
            }
            None => {
                ui.ctx()
                    .data_mut(|data| data.remove_temp::<(usize, f32)>(curve_drag_id));
            }
        }
    }
    let curve_drag: Option<(usize, f32)> = ui.ctx().data(|data| data.get_temp(curve_drag_id));
    if curve_modifier_held
        && response.dragged()
        && let Some((index, initial)) = curve_drag
        && let (Some(pos), Some(origin)) = (
            response.interact_pointer_pos(),
            ui.input(|i| i.pointer.press_origin()),
        )
    {
        // Absolute from the press, like every drag here: `drag_delta` is
        // the delta since the last FRAME, and rebuilding from `initial`
        // each frame would throw the previous frames' movement away — a
        // slow drag would twitch and stay flat.
        automation.points_mut(target)[index].bend =
            (initial - (pos.y - origin.y) / rect.height() * 2.0).clamp(-1.0, 1.0);
    }
    if curve_modifier_held
        && response.double_clicked()
        && let Some(index) = curve_hit
    {
        automation.points_mut(target)[index].bend = 0.0;
    }
    let delete_point = std::cell::Cell::new(None);
    for (index, point) in automation.points_mut(target).iter_mut().enumerate() {
        let pos = point_pos(point.beat, point.value);
        let hit = egui::Rect::from_center_size(pos, egui::Vec2::splat(8.0));
        let response = ui
            .interact(hit, id.with(index), egui::Sense::click_and_drag())
            .affords(Affords::Steer);
        hovered |= response.hovered();
        painter.circle_filled(
            pos,
            3.0,
            if response.hovered() {
                theme.text
            } else {
                theme.accent
            },
        );
        if response.hovered() {
            painter.circle_stroke(pos, 5.5, egui::Stroke::new(1.5, theme.text));
            painter.text(
                pos + egui::vec2(7.0, -7.0),
                egui::Align2::LEFT_BOTTOM,
                "double-click to delete",
                egui::FontId::proportional(9.0),
                theme.text,
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.dragged() {
            let to = pos + response.drag_delta();
            let freehand = ui.input(|i| i.modifiers.alt);
            let raw_beat = beat_at(rect, view_beats, pixels_per_beat, to.x).max(0.0);
            point.beat = if freehand {
                raw_beat
            } else {
                snap(raw_beat, grid)
            };
            point.value = value_at_y(to.y);
        }
        if response.double_clicked() {
            delete_point.set(Some(index));
        }
    }
    let points = automation.points_mut(target);
    if let Some(index) = delete_point.get() {
        points.remove(index);
    }
    points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    hovered
}

pub(crate) fn parameter_base(track: &Track, target: &str, spec: &ParameterSpec) -> f32 {
    use daw::params::pan;
    match target_ref(target) {
        Some(TargetRef::TrackOutput(pan::GAIN)) => track.volume,
        Some(TargetRef::TrackOutput(_)) => track.pan,
        Some(TargetRef::Device { id, param }) => track
            .device(id)
            .and_then(|instance| instance.state.value(param))
            .unwrap_or(spec.default),
        None => spec.default,
    }
}

/// Whether a target means anything ON THIS TRACK: a curve aimed at a device
/// this lane does not carry would automate silence, and the picker should
/// not offer it. Track-group targets apply everywhere.
pub(crate) fn target_applies(track: &Track, target: &str) -> bool {
    match target_ref(target) {
        Some(TargetRef::TrackOutput(_)) => true,
        Some(TargetRef::Device { id, param }) => track
            .device(id)
            .is_some_and(|instance| instance.state.value(param).is_some()),
        None => false,
    }
}

/// Where a target's automated value lands in the ENGINE: which node, and
/// which `ParamChange` id on it. One function, total over every target the
/// app can build, so playback dispatch cannot silently miss one — the test
/// walks the whole device table through it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TargetRef {
    /// The track's output stage (`pans`), which every audible track has.
    TrackOutput(u32),
    /// A device INSTANCE, by the id its target names.
    Device { id: u64, param: u32 },
}

pub(crate) fn target_ref(target: &str) -> Option<TargetRef> {
    use daw::params::pan;
    match target {
        TRACK_VOLUME_TARGET => return Some(TargetRef::TrackOutput(pan::GAIN)),
        TRACK_PAN_TARGET => return Some(TargetRef::TrackOutput(pan::PAN)),
        _ => {}
    }
    // `dev.<id>.<prefix>.<param>`. A target the tables do not recognize is
    // no target at all — a renamed prefix orphans its wires rather than
    // aiming them at whatever now sits in that slot.
    let (id, rest) = target.strip_prefix(DEVICE_TARGET_PREFIX)?.split_once('.')?;
    let (prefix, name) = rest.split_once('.')?;
    let param = device_by_prefix(prefix)?
        .params
        .iter()
        .find(|def| def.name == name)?;
    Some(TargetRef::Device {
        id: id.parse().ok()?,
        param: param.id,
    })
}

/// Registry-backed chooser. Group headings and stable ids are already part
/// of the contract; a future device only registers specs and appears here.
pub(crate) fn automation_target_picker(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    prefix: &str,
    registry: &ParameterRegistry,
    // The track the picker is choosing FOR, when one is under the hand:
    // targets whose device the track does not carry are not offered — a
    // synth curve on a synthless track automates silence.
    track: Option<&Track>,
    target: &mut String,
) {
    let selected = registry
        .spec(target)
        .map_or_else(|| target.clone(), |spec| spec.name.clone());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let response = child.add_sized(
        rect.size(),
        egui::Button::new(format!("{prefix}{selected} ▾")),
    );
    egui::Popup::menu(&response)
        .id(id.with("popup"))
        .show(|ui| {
            let query_id = id.with("query");
            let mut query: String = ui
                .ctx()
                .data(|data| data.get_temp(query_id).unwrap_or_default());
            ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text("Search parameters")
                    .desired_width(210.0),
            );
            ui.ctx()
                .data_mut(|data| data.insert_temp(query_id, query.clone()));
            let query = query.trim().to_lowercase();
            // With a track under the hand the list is that lane's LIVE
            // targets, instance ids and all. Without one there is no chain
            // to name, so only the pair every track has can be offered.
            let entries = match track {
                Some(track) => track_targets(track, registry),
                None => [TRACK_VOLUME_TARGET, TRACK_PAN_TARGET]
                    .into_iter()
                    .filter_map(|id| {
                        registry.spec(id).map(|spec| TargetEntry {
                            id: id.to_owned(),
                            group: spec.group.clone(),
                            name: spec.name.clone(),
                        })
                    })
                    .collect(),
            };
            let mut last_group = String::new();
            for entry in entries {
                if !query.is_empty()
                    && !entry.name.to_lowercase().contains(&query)
                    && !entry.group.to_lowercase().contains(&query)
                    && !entry.id.to_lowercase().contains(&query)
                {
                    continue;
                }
                if entry.group != last_group {
                    if !last_group.is_empty() {
                        ui.separator();
                    }
                    ui.label(&entry.group);
                    last_group = entry.group.clone();
                }
                let unit = registry
                    .spec(&entry.id)
                    .map_or_else(String::new, |spec| spec.unit.clone());
                let suffix = if unit.is_empty() {
                    String::new()
                } else {
                    format!("  {unit}")
                };
                if ui
                    .selectable_label(target == &entry.id, format!("{}{}", entry.name, suffix))
                    .clicked()
                {
                    *target = entry.id.clone();
                    egui::Popup::close_all(ui.ctx());
                }
            }
        });
}

/// A focused, full-height view of the automation currently under the hand.
/// The compact track sublane is for quick moves; this is the place to make
/// deliberate shapes without clips or track controls competing for space.
pub(crate) fn automation_editor_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    arr: &mut Arrangement,
    beats_per_bar: u32,
    registry: &ParameterRegistry,
    automation_target: &mut String,
) -> bool {
    let area = ui.max_rect();
    claim(ui);
    ui.painter().rect_filled(area, 0.0, theme.bg);

    const HEADER_H: f32 = 30.0;
    let header = egui::Rect::from_min_size(area.min, egui::vec2(area.width(), HEADER_H));
    let editor = egui::Rect::from_min_max(
        egui::pos2(area.left(), header.bottom()),
        area.right_bottom(),
    );
    ui.painter().rect_filled(header, 0.0, theme.surface_sunken);
    ui.painter().line_segment(
        [header.left_bottom(), header.right_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    let close = egui::Rect::from_min_size(
        egui::pos2(header.left() + 6.0, header.top() + 5.0),
        egui::vec2(88.0, header.height() - 10.0),
    );
    let close_response = ui
        .interact(
            close,
            ui.id().with("close_automation_editor"),
            egui::Sense::click(),
        )
        .affords(Affords::Press);
    ui.painter().rect_filled(
        close,
        0.0,
        if close_response.hovered() {
            theme.surface
        } else {
            theme.surface_sunken
        },
    );
    ui.painter().text(
        close.center(),
        egui::Align2::CENTER_CENTER,
        "← TIMELINE",
        egui::FontId::proportional(10.0),
        theme.text,
    );

    let selected = arr
        .selected
        .and_then(|i| arr.tracks.get(i).map(|track| (i, track.name.clone())));
    if let Some((track_index, track_name)) = selected {
        let target_button = egui::Rect::from_min_size(
            egui::pos2(close.right() + 8.0, close.top()),
            egui::vec2(112.0, close.height()),
        );
        automation_target_picker(
            ui,
            target_button,
            ui.id().with("automation_editor_target"),
            "",
            registry,
            arr.tracks.get(track_index),
            automation_target,
        );
        ui.painter().text(
            egui::pos2(target_button.right() + 10.0, header.center().y),
            egui::Align2::LEFT_CENTER,
            format!(
                "{}  —  click to add • ⌥ point drag: off-grid • ⌥ line drag: bend • ⌥ double-click: straighten • double-click point: delete",
                track_name
            ),
            egui::FontId::proportional(11.0),
            theme.text_muted,
        );

        // More pixels per beat and the whole central panel gives curves the
        // working room they need, while the compact lane remains unchanged.
        let focused_ppb =
            |base: f32| (base * 2.0).clamp(ARRANGEMENT_ZOOM_MIN, ARRANGEMENT_ZOOM_MAX);
        let mut focused_pixels_per_beat = focused_ppb(arr.pixels_per_beat);

        // The editor is a view over the SAME time axis as the timeline: the
        // wheel pans it and ctrl+scroll (or pinch) zooms it anchored under
        // the pointer, and both write straight back to the shared view — so
        // leaving focused mode lands the timeline where you left the curve.
        let pointer_here = ui
            .ctx()
            .pointer_latest_pos()
            .is_some_and(|pos| editor.contains(pos));
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        let pan = scroll.x + scroll.y;
        if pan != 0.0 && pointer_here {
            arr.view_beats = (arr.view_beats + pan / focused_pixels_per_beat).max(0.0);
        }
        let zoom = ui.input(|i| i.zoom_delta());
        if zoom != 1.0
            && pointer_here
            && let Some(pos) = ui.ctx().pointer_latest_pos()
        {
            let old = focused_pixels_per_beat;
            arr.pixels_per_beat =
                (arr.pixels_per_beat * zoom).clamp(ARRANGEMENT_ZOOM_MIN, ARRANGEMENT_ZOOM_MAX);
            focused_pixels_per_beat = focused_ppb(arr.pixels_per_beat);
            if focused_pixels_per_beat != old {
                // The beat under the pointer stays under the pointer, at
                // the FOCUSED scale on both sides of the change.
                let anchor = arr.view_beats + (pos.x - editor.left()) / old;
                arr.view_beats =
                    (anchor - (pos.x - editor.left()) / focused_pixels_per_beat).max(0.0);
            }
        }

        beat_grid(
            ui,
            editor,
            theme,
            arr,
            beats_per_bar,
            focused_pixels_per_beat,
        );
        let Some(spec) = registry.spec(automation_target) else {
            return close_response.clicked();
        };
        let base = parameter_base(&arr.tracks[track_index], automation_target, spec);
        automation_lane(
            ui,
            editor,
            theme,
            arr.view_beats,
            focused_pixels_per_beat,
            arr.grid_beats(),
            automation_target,
            spec,
            &mut arr.tracks[track_index].automation,
            base,
        );
    } else {
        kit::empty_state(ui, theme, "select a track to edit automation");
    }
    close_response.clicked()
}

/// The arrangement: a loop ruler, then lanes stacked in a beat grid.
///
/// Selection is click-and-drag inside a lane; it snaps to the grid and stays
/// within the one track, because a selection spanning lanes would have no
/// meaning for the loop it becomes.
///
/// The view pans horizontally with the wheel (Shift+wheel or plain wheel —
/// there is no vertical overflow to spend it on). Returns true when the user
/// panned this frame, so the caller can hand the view back: manual panning
/// turns follow off.
///
/// `playhead` is the transport position in beats; it is drawn above the
/// clips, and `follow` pages the view to keep it on screen.
#[derive(Clone, Copy)]
pub(crate) struct ArrangementTransportView {
    pub(crate) beats_per_bar: u32,
    pub(crate) bpm: f64,
    pub(crate) playhead: f32,
    pub(crate) follow: bool,
}

#[derive(Default)]
pub(crate) struct ArrangementOutcome {
    pub(crate) panned: bool,
    pub(crate) automation_hovered: bool,
    /// A clip body was double-clicked and the lower region should switch
    /// from the rack to the editor appropriate for that clip's track.
    pub(crate) open_clip_editor: bool,
    /// A fade handle on a timeline clip was dragged: (clip id, the edit).
    pub(crate) clip_fade: Option<(u64, waveform::ClipEdit)>,
}

// The timeline composes independent UI services at the panel boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn arrangement_body(
    ui: &mut egui::Ui,
    focus: &mut Focus,
    theme: &Theme,
    arr: &mut Arrangement,
    transport: ArrangementTransportView,
    waveform_cache: &HashMap<PathBuf, Arc<waveform::Peaks>>,
    drag: Option<&mut DragImport>,
    automation_mode: bool,
    registry: &ParameterRegistry,
    automation_target: &mut String,
    meters: &mut Vec<device::meter::Ballistics>,
    master_meter: &mut device::meter::Ballistics,
) -> ArrangementOutcome {
    let ArrangementTransportView {
        beats_per_bar,
        bpm,
        playhead,
        follow,
    } = transport;
    let area = ui.max_rect();
    claim(ui);

    // Wheel panning. Follow is turned off by the caller on the return value,
    // not here — the arrangement does not own transport state.
    // The header column owns the left edge; everything time-shaped lives to
    // the right of it. Splitting HERE, before anything reads a rect, is what
    // keeps `beat_at` / `x_at` honest: they measure from `content.left()`,
    // so the timeline's beat 0 is the column's right edge and not the
    // window's.
    let column_w = HEADER_W.min(area.width() * 0.5);
    let timeline_left = area.left() + column_w;

    let scroll = ui.input(|i| i.smooth_scroll_delta);
    let mut panned = false;
    let mut automation_hovered = false;
    let pointer_on_timeline = ui
        .ctx()
        .pointer_latest_pos()
        .is_some_and(|p| p.x >= timeline_left);
    let on_timeline = ui.ui_contains_pointer() && pointer_on_timeline;
    // ONE AXIS EACH, where both used to fold into the horizontal pan.
    //
    // egui has already sorted the axes out by the time this reads them:
    // it folds a wheel into `x` while the horizontal modifier is held
    // (Shift, by default) and into `y` otherwise, and Ctrl+wheel it takes
    // away entirely as `zoom_delta`. So the wheel is vertical, Shift+wheel
    // is horizontal, and neither has to know about the other.
    let (sideways, down) = wheel_axes(scroll, arr.pixels_per_beat);
    if sideways != 0.0 && on_timeline {
        arr.view_beats = (arr.view_beats + sideways).max(0.0);
        panned = true;
    }
    if down != 0.0 && on_timeline {
        // NOT `panned`: that turns FOLLOW off, and follow is about
        // keeping the playhead on screen as time passes. Scrolling to
        // another lane is not taking the wheel away from it.
        arr.scroll_tracks_to(arr.view_tracks_y + down);
    }

    // Ctrl+scroll (and pinch) zooms, anchored under the pointer: the beat
    // being pointed at stays put while the scale changes around it. egui
    // folds ctrl+scroll into `zoom_delta` and out of the scroll delta, so
    // a zoom is never also a pan.
    let zoom = ui.input(|i| i.zoom_delta());
    if zoom != 1.0
        && ui.ui_contains_pointer()
        && pointer_on_timeline
        && let Some(pos) = ui.ctx().pointer_latest_pos()
    {
        let old = arr.pixels_per_beat;
        let new = (old * zoom).clamp(ARRANGEMENT_ZOOM_MIN, ARRANGEMENT_ZOOM_MAX);
        if new != old {
            let anchor = arr.view_beats + (pos.x - timeline_left) / old;
            arr.pixels_per_beat = new;
            arr.view_beats = (anchor - (pos.x - timeline_left) / new).max(0.0);
            // Zooming is the user taking the wheel the same way panning is;
            // follow would page the anchored view right back out.
            panned = true;
        }
    }

    let minimap = egui::Rect::from_min_max(
        egui::pos2(timeline_left, area.top()),
        egui::pos2(area.right(), area.top() + MINIMAP_H),
    );
    let ruler = egui::Rect::from_min_max(
        egui::pos2(timeline_left, minimap.bottom()),
        egui::pos2(area.right(), minimap.bottom() + LOOP_RULER_H),
    );
    // The master takes a row across the FOOT of the arrangement — header
    // on the left, an empty strip beside it where its own automation will
    // go. Reserved before anything else measures, so the lane stack and
    // the header column agree about where the lanes stop: a lane visible
    // in the grid with its header hidden under the master is the kind of
    // mismatch that reads as a drawing bug.
    let lanes_bottom = (area.bottom() - MASTER_HEAD_H).max(ruler.bottom());
    let master_row = egui::Rect::from_min_max(egui::pos2(area.left(), lanes_bottom), area.max);
    let content = egui::Rect::from_min_max(
        egui::pos2(timeline_left, ruler.bottom()),
        egui::pos2(area.right(), lanes_bottom),
    );
    arr.viewport_width = content.width();
    arr.viewport_height = content.height();
    // The wheel above ran against LAST frame's height — on the very first
    // frame there was none, and a clamp against a zero viewport would let
    // the stack scroll clean off its own bottom. Re-clamping here is
    // idempotent once the figure is right.
    arr.scroll_tracks_to(arr.view_tracks_y);
    let column = egui::Rect::from_min_max(
        egui::pos2(area.left(), ruler.bottom()),
        egui::pos2(timeline_left, lanes_bottom),
    );
    let grid = arr.grid_beats();

    // Follow pages the view BEFORE anything reads the offset, so the
    // playhead is on screen the same frame it outran the window.
    arr.view_beats = follow_view(
        arr.view_beats,
        playhead,
        content.width() / arr.pixels_per_beat,
        follow,
    );

    // The minimap runs BEFORE `offset` is read: a jump or scrub there must
    // move the content this same frame, not one frame late.
    if minimap_pass(ui, theme, minimap, arr, content.width(), playhead) {
        panned = true;
    }

    // While session clips override the timeline, the timeline says so — a
    // lit chip over the minimap's corner, and clicking it is the way back.
    // Created AFTER the minimap's interact, so the chip wins the pointer
    // where they overlap.
    if arr.session.playing.iter().any(Option::is_some) {
        let chip = egui::Rect::from_min_size(
            egui::pos2(minimap.right() - 118.0, minimap.top() + 2.0),
            egui::vec2(116.0, 14.0),
        );
        let wid = ui.id().with("timeline_back_to_arr");
        let response = ui
            .interact(chip, wid, egui::Sense::click())
            .affords(Affords::Press);
        if response.clicked() {
            arr.force_recompile |= arr.session.stop_all();
        }
        let painter = ui.painter();
        painter.rect_filled(
            chip,
            0.0,
            if response.hovered() {
                theme.warn
            } else {
                theme.warn.gamma_multiply(0.75)
            },
        );
        painter.text(
            chip.center(),
            egui::Align2::CENTER_CENTER,
            "session playing — click to return",
            egui::FontId::proportional(9.0),
            theme.bg,
        );
    }
    let offset = arr.view_beats;

    let lanes = lane_rects(content, &arr.tracks, arr.view_tracks_y);
    paint_lane_bands(ui, content, &lanes, arr.selected, theme);
    beat_grid(ui, content, theme, arr, beats_per_bar, arr.pixels_per_beat);
    arrangement_ruler(
        ui,
        ruler,
        content,
        theme,
        arr,
        beats_per_bar,
        arr.pixels_per_beat,
    );

    ui.painter().text(
        egui::pos2(area.right() - GRID_LABEL_PAD, ruler.center().y),
        egui::Align2::RIGHT_CENTER,
        GRID_NAMES[arr.grid.min(GRID_NAMES.len() - 1)],
        egui::FontId::new(GRID_LABEL_TYPE, egui::FontFamily::Monospace),
        theme.text_muted,
    );
    if automation_mode {
        let badge = egui::Rect::from_min_size(
            egui::pos2(ruler.left() + 4.0, ruler.top() + 2.0),
            egui::vec2(124.0, ruler.height() - 4.0),
        );
        automation_target_picker(
            ui,
            badge,
            ui.id().with("automation_target"),
            "AUTO: ",
            registry,
            arr.selected.and_then(|track| arr.tracks.get(track)),
            automation_target,
        );
    }

    // --- the loop region, under everything it covers ----------------------
    if let Some((from, to)) = arr.loop_range {
        let band = egui::Rect::from_min_max(
            egui::pos2(
                x_at(content, offset, arr.pixels_per_beat, from),
                content.top(),
            ),
            egui::pos2(
                x_at(content, offset, arr.pixels_per_beat, to),
                content.bottom(),
            ),
        );
        ui.painter()
            .rect_filled(band.intersect(content), 0.0, theme.loop_region);
    }

    let grab = ui.style().interaction.resize_grab_radius_side;
    let mut resize: Option<(usize, f32)> = None;
    // The anchor lane, the lane the pointer is over now, and the span:
    // a band is a RECTANGLE of tracks and time, so the drag reports both
    // axes and the anchor is what the second one is measured from.
    let mut select: Option<(usize, f32, f32)> = None;
    let mut band_to: Option<usize> = None;
    let mut seek_req: Option<f32> = None;
    let mut create_req: Option<(usize, f32)> = None;
    // A Cell for the same reason as clips_pass's menu: one closure per lane
    // per frame, one shared slot.
    let menu_create: std::cell::Cell<Option<(usize, f32)>> = std::cell::Cell::new(None);
    // Where the cell cursor sits. Beat is shared across lanes so moving up
    // or down keeps your place in time.
    let (_, cursor_beat) = arr.cursor.unwrap_or((0, 0.0));
    let mut focused_lane: Option<usize> = None;

    for (i, lane) in lanes.iter().enumerate() {
        if lane.top() > content.bottom() {
            break;
        }
        let visible = lane.intersect(content);
        // The focusable rect is the CELL, not the lane. Cells in different
        // lanes share an x, so Up and Down keep your place in time while the
        // generic spatial navigation does the work.
        let cell = cell_rect(
            content,
            offset,
            arr.pixels_per_beat,
            visible,
            cursor_beat,
            grid,
        );
        let wid = ui.id().with(("lane", i));
        if focus.register(wid, cell.intersect(visible)) {
            focused_lane = Some(i);
        }

        let shows_automation = automation_mode && arr.selected == Some(i);

        // The lane body excludes both the automation sublane and the resize
        // strip. An automation gesture therefore cannot also select time or
        // create a clip on a double click.
        let body_bottom = visible.bottom()
            - if shows_automation {
                AUTOMATION_LANE_H
            } else {
                0.0
            };
        let body = egui::Rect::from_min_max(
            visible.min,
            egui::pos2(visible.right(), (body_bottom - grab).max(visible.top())),
        );
        let anchor_id = wid.with("anchor");
        let picked = ui
            .interact(body, wid.with("body"), egui::Sense::click_and_drag())
            .affords(Affords::Carry);
        if let Some(pos) = picked.interact_pointer_pos() {
            let here = snap(beat_at(content, offset, arr.pixels_per_beat, pos.x), grid);
            if picked.drag_started() || picked.clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(anchor_id, here));
                select = Some((i, here, here));
                band_to = Some(i);
                seek_req = Some(here);
            } else if picked.dragged() {
                let from: f32 = ui.ctx().data(|d| d.get_temp(anchor_id).unwrap_or(here));
                select = Some((i, from, here));
                // WHICH LANE THE POINTER IS OVER, not which lane owns
                // the drag. egui keeps the interaction with the lane the
                // press landed on — which is what makes the gesture
                // survive leaving it — so the second axis has to come
                // from geometry or the band could never grow past one
                // track.
                band_to = lane_at(&lanes, pos.y).or(Some(i));
            }
        }

        // Double-click creates a one-bar clip at the click; the right-click
        // menu offers the same through words.
        if picked.double_clicked()
            && let Some(pos) = picked.interact_pointer_pos()
        {
            create_req = Some((
                i,
                snap(beat_at(content, offset, arr.pixels_per_beat, pos.x), grid),
            ));
        }
        let menu_beat = picked
            .interact_pointer_pos()
            .map(|p| snap(beat_at(content, offset, arr.pixels_per_beat, p.x), grid))
            .unwrap_or(cursor_beat);
        picked.context_menu(|ui| {
            if ui.button("New clip").clicked() {
                menu_create.set(Some((i, menu_beat)));
                ui.close();
            }
        });

        // The selection wash, on every lane the band covers.
        //
        // It used to be `arr.selected == Some(i)` — the anchor lane
        // alone — so a marquee dragged down three tracks selected their
        // clips and showed a wash on one. The band is a rectangle now,
        // and it has to look like one.
        let in_band = arr
            .selection_band()
            .is_some_and(|(first, last)| (first..=last).contains(&i));
        if in_band && let Some((from, to)) = arr.selection {
            let band = egui::Rect::from_min_max(
                egui::pos2(
                    x_at(content, offset, arr.pixels_per_beat, from),
                    visible.top(),
                ),
                egui::pos2(
                    x_at(content, offset, arr.pixels_per_beat, to),
                    visible.bottom(),
                ),
            );
            ui.painter()
                .rect_filled(band.intersect(body), 0.0, theme.selection);
            for beat in [from, to] {
                let x = x_at(content, offset, arr.pixels_per_beat, beat);
                if body.left() <= x && x <= body.right() {
                    ui.painter().line_segment(
                        [egui::pos2(x, body.top()), egui::pos2(x, body.bottom())],
                        egui::Stroke::new(stroke::HAIR, theme.accent),
                    );
                }
            }
        }

        if shows_automation && let Some(spec) = registry.spec(automation_target) {
            let base = parameter_base(&arr.tracks[i], automation_target, spec);
            automation_hovered |= automation_lane(
                ui,
                automation_rect(visible),
                theme,
                offset,
                arr.pixels_per_beat,
                grid,
                automation_target,
                spec,
                &mut arr.tracks[i].automation,
                base,
            );
        }

        // The boundary below this lane resizes it.
        let seam = egui::Rect::from_min_max(
            egui::pos2(lane.left(), lane.bottom() - grab),
            egui::pos2(lane.right(), lane.bottom() + grab),
        );
        let response = ui
            .interact(seam, wid.with("seam"), egui::Sense::drag())
            .affords(Affords::SeamY);
        if response.dragged() {
            resize = Some((i, arr.tracks[i].height + response.drag_delta().y));
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(lane.left(), lane.bottom() - SEAM_PX),
                    egui::pos2(lane.right(), lane.bottom()),
                ),
                0.0,
                theme.focus,
            );
        } else {
            ui.painter().line_segment(
                [
                    egui::pos2(lane.left(), lane.bottom()),
                    egui::pos2(lane.right(), lane.bottom()),
                ],
                egui::Stroke::new(1.0, theme.divider),
            );
        }
    }

    // THE BAND'S OWN EDGE, drawn once around the whole rectangle rather
    // than per lane.
    //
    // The wash says which time and which tracks; the corners say where
    // the gesture ENDS, which a wash bleeding off the top and bottom of
    // the screen cannot. Brackets rather than a closed box for the
    // reason `ui::hud` gives: a rectangle drawn around a region covers
    // the region's own edges, and here those are the lane rules the
    // arrangement is read by.
    if let (Some((from, to)), Some((first, last))) = (arr.selection, arr.selection_band())
        && to > from
        && let (Some(top), Some(bottom)) = (lanes.get(first), lanes.get(last))
    {
        let band = egui::Rect::from_min_max(
            egui::pos2(x_at(content, offset, arr.pixels_per_beat, from), top.top()),
            egui::pos2(
                x_at(content, offset, arr.pixels_per_beat, to),
                bottom.bottom(),
            ),
        )
        .intersect(content);
        if band.width() > 0.0 && band.height() > 0.0 {
            daw::ui::hud::brackets(
                ui.painter(),
                band,
                egui::Stroke::new(stroke::BOLD, theme.accent),
            );
            // HOW LONG IT IS.
            //
            // The one number a selection owes you and the only one it
            // could not give: every time operation is defined over this
            // span, and working out "is that four bars or five" by
            // counting grid lines is the sort of arithmetic an interface
            // is supposed to have already done.
            //
            // Inside the band's own top-left corner, because that is
            // where the gesture began and where the eye already is.
            let text = beats_as_bars(to - from, beats_per_bar);
            let at = egui::pos2(band.left() + BAND_TAG_PAD, band.top() + BAND_TAG_PAD);
            if band.width() > BAND_TAG_MIN_W && band.height() > BAND_TAG_MIN_H {
                ui.painter().text(
                    at,
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::monospace(font::MINI_LABEL),
                    theme.accent,
                );
            }
        }
    }

    if let Some((i, from, to)) = select {
        arr.select_track(i);
        let span = span(from, to, grid);
        arr.selection = Some(span);
        let reach = band_to.unwrap_or(i);
        arr.selection_tracks = Some((i.min(reach), i.max(reach)));
        // Keep the keyboard where the mouse just went, so arrowing carries
        // on from where you clicked instead of jumping back.
        arr.cursor = Some((i, span.0));
        arr.anchor = span.0;
        // A drag on empty lane is a new intention; a clip left selected
        // from before is not part of it.
        //
        // What the band DOES cover becomes the selection, so every
        // clip command already built — delete, copy, nudge, group drag —
        // works on a marquee without knowing one exists.
        arr.selected_clip = None;
        arr.selected_clip_ids.clear();
        let caught = clips_in_band(&arr.clips, (i.min(reach), i.max(reach)), span.0, span.1);
        if let Some((track, index)) = caught.first().copied() {
            arr.selected_clip = Some((track, index));
            arr.selected_clip_ids = caught
                .iter()
                .skip(1)
                .filter_map(|(track, index)| arr.clips.get(*track)?.get(*index).map(|c| c.id))
                .collect();
        }
    }
    // The press IS the insert marker: Play starts where you pointed.
    // Press only — a drag-select must not scrub the song along behind
    // the selection — and it moves the PLAYHEAD only when the transport
    // is stopped, which is what `pending_point` is for.
    if let Some(beat) = seek_req {
        arr.pending_point = Some(beat);
    }

    // Arrowing between lanes moves the cursor and takes the selection with
    // it — the cell you are on IS the selection.
    arr.owns_arrows = focused_lane.is_some();
    if let Some(i) = focused_lane
        && arr.cursor.map(|c| c.0) != Some(i)
    {
        arr.cursor = Some((i, cursor_beat));
        arr.anchor = cursor_beat;
        arr.select_track(i);
        arr.selection = Some(span(cursor_beat, cursor_beat, grid));
    }
    if let Some((i, height)) = resize {
        arr.tracks[i].height = height.clamp(*TRACK_H_RANGE.start(), *TRACK_H_RANGE.end());
    }

    // Creation: double-click and the menu both land here, one bar long, at
    // the snapped click beat. `create_clip` picks the first gap that fits
    // and selects the result.
    if let Some((t, at)) = create_req.or(menu_create.get())
        && arr.create_clip(t, at, beats_per_bar as f32).is_some()
    {
        arr.select_track(t);
    }

    // The scrub strip: the ruler's empty stretches jump the transport to
    // the snapped click. Created BEFORE the brace and the locators, so
    // both of those win the pointer where they overlap it — an empty
    // stretch is exactly the part neither of them claims.
    let scrub = ui
        .interact(ruler, ui.id().with("scrub"), egui::Sense::click())
        .affords(Affords::Press);
    // Where a click would LAND, drawn under the pointer before it is
    // spent. The ruler SNAPS, so the honest preview is the snapped beat
    // and not the pointer's own x — otherwise every seek arrives a
    // little off where it was aimed and the grid takes the blame.
    if let Some(pos) = scrub.hover_pos() {
        let at = snap(beat_at(content, offset, arr.pixels_per_beat, pos.x), grid);
        let x = x_at(content, offset, arr.pixels_per_beat, at);
        if ruler.x_range().contains(x) {
            ui.painter().line_segment(
                [
                    egui::pos2(x, ruler.top() + 2.0),
                    egui::pos2(x, ruler.bottom()),
                ],
                egui::Stroke::new(stroke::HAIR, theme.text_muted),
            );
        }
    }
    if scrub.clicked()
        && let Some(pos) = scrub.interact_pointer_pos()
    {
        arr.pending_seek = Some(snap(
            beat_at(content, offset, arr.pixels_per_beat, pos.x),
            grid,
        ));
    }
    loop_brace(ui, theme, focus, ruler, content, arr, grid);
    locators_pass(ui, theme, ruler, content, arr, grid);
    let clips = clips_pass(
        ui,
        theme,
        content,
        arr,
        grid,
        bpm,
        waveform_cache,
        automation_mode,
    );

    // The drop ghost, above the clips it would join and under the playhead.
    // The spot it lands on rides back out on the drag, where the app reads
    // it when the file is released.
    if let Some(drag) = drag {
        drag.spot = drop_preview(
            ui,
            theme,
            content,
            arr,
            grid,
            beats_per_bar,
            bpm,
            waveform_cache,
            drag,
        );
    }

    // The headers last of the lane furniture, so their fills and controls
    // sit above the grid lines that run under the column's edge.
    track_headers(ui, theme, arr, column, &lanes, meters);
    master_header(ui, theme, arr, master_row, timeline_left, master_meter);

    // --- the playhead, above everything it passes over ---------------------
    let x = x_at(content, offset, arr.pixels_per_beat, playhead);
    if x >= content.left() && x <= content.right() {
        let painter = ui.painter();
        painter.line_segment(
            [
                egui::pos2(x + 1.0, content.top()),
                egui::pos2(x + 1.0, content.bottom()),
            ],
            egui::Stroke::new(stroke::BOLD, theme.bg.gamma_multiply(0.65)),
        );
        painter.line_segment(
            [
                egui::pos2(x, content.top()),
                egui::pos2(x, content.bottom()),
            ],
            egui::Stroke::new(1.5, theme.playhead),
        );
        // The ruler triangle, pointing down at the line it belongs to.
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x, ruler.top() + 2.0),
                egui::pos2(x - PLAYHEAD_TRI_HALF, ruler.top() + 2.0 + PLAYHEAD_TRI_H),
                egui::pos2(x + PLAYHEAD_TRI_HALF, ruler.top() + 2.0 + PLAYHEAD_TRI_H),
            ],
            theme.playhead,
            egui::Stroke::NONE,
        ));
    }

    ArrangementOutcome {
        panned,
        automation_hovered,
        open_clip_editor: clips.open_editor,
        clip_fade: clips.fade,
    }
}

/// Session grid metrics.
pub(crate) const SLOT_H: f32 = 24.0;
pub(crate) const SLOT_GAP: f32 = 2.0;
/// The launch triangle's column inside a slot: the part that STARTS a clip,
/// as against the rest of the slot, which selects it.
pub(crate) const SLOT_LAUNCH_W: f32 = 18.0;
pub(crate) const SESSION_COL_MIN: f32 = 78.0;
pub(crate) const SESSION_COL_MAX: f32 = 170.0;
/// The scene column down the right: launch buttons and row names.
pub(crate) const SCENE_COL_W: f32 = 128.0;
/// The master strip's column, pinned between the lanes and the scenes.
/// Narrower than a lane on purpose: it holds one fader and one meter, and
/// the space it takes is space the song does not get.
pub(crate) const MASTER_COL_W: f32 = 92.0;
/// The column header, showing the track's name.
pub(crate) const SESSION_HEAD_H: f32 = 22.0;
/// The mixer strip under each column: mute and solo, a pan knob, and the
/// channel assembly — the segmented meter living inside the fader's own
/// track, the way Ableton's session mixer draws it — with the level in a
/// readout box underneath. Tall enough that the fader has travel worth
/// having.
pub(crate) const SESSION_MIXER_H: f32 = 128.0;
/// The channel assembly's width. Thick: the fader IS the meter's track,
/// and both are the strip's centrepiece rather than furniture at its edge.
pub(crate) const ASSEMBLY_W: f32 = 26.0;
/// The latched clip lamp above the assembly, clicked to clear.
pub(crate) const CLIP_LAMP_H: f32 = 5.0;
/// The mixer section's height range: enough for the buttons alone at the
/// bottom end, a long-throw fader at the top.
pub(crate) const SESSION_MIXER_H_RANGE: std::ops::RangeInclusive<f32> = 56.0..=280.0;
/// The draggable seam on the mixer section's top edge.
pub(crate) const MIXER_SEAM_H: f32 = 5.0;
/// The horizontal scrollbar under the columns, shown only when the grid is
/// wider than the window.
pub(crate) const SESSION_BAR_H: f32 = 7.0;

/// The minimap: the whole arrangement mapped into one strip, every clip a
/// minified bar on its lane's row, the loop region and playhead behind and
/// through them, and the current view as a window on top.
///
/// A press inside the window grabs it and drags it — absolute from the
/// pointer, keeping the grip point, like every other drag here. A press
/// outside centres the window on the pointer and scrubs from there.
/// Returns true when the user moved the view; the caller treats that like
/// a pan and releases playhead follow.
pub(crate) fn minimap_pass(
    ui: &mut egui::Ui,
    theme: &Theme,
    strip: egui::Rect,
    arr: &mut Arrangement,
    content_width: f32,
    playhead: f32,
) -> bool {
    if strip.width() <= 0.0 || strip.height() <= 0.0 {
        return false;
    }
    // The window the timeline currently shows, in beats.
    let viewport = content_width / arr.pixels_per_beat;
    // The mapped span: everything that exists plus headroom, and never less
    // than the current window — the window must always fit inside the map.
    let clips_end = arr
        .clips
        .iter()
        .flatten()
        .map(|clip| clip.start + clip.len)
        .fold(0.0f32, f32::max);
    let loop_end = arr.loop_range.map_or(0.0, |(_, to)| to);
    let span = (clips_end.max(loop_end).max(playhead) * 1.05)
        .max(arr.view_beats + viewport)
        .max(viewport);
    let scale = strip.width() / span;

    // Interact before painting, so the drawn window is where this frame's
    // grab put it.
    let id = ui.id().with("minimap");
    let resp = ui
        .interact(strip, id, egui::Sense::click_and_drag())
        .affords(Affords::Sweep);
    if resp.hovered() && !resp.is_pointer_button_down_on() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let mut moved = false;
    if resp.is_pointer_button_down_on()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let beat = (pos.x - strip.left()) / scale;
        if ui.input(|i| i.pointer.primary_pressed()) {
            // Where in the window the press landed — the grip the whole
            // drag keeps. A press outside grips the window's centre, which
            // is what centres it on the pointer.
            let inside = (arr.view_beats..arr.view_beats + viewport).contains(&beat);
            let grab = if inside {
                beat - arr.view_beats
            } else {
                viewport * 0.5
            };
            ui.data_mut(|d| d.insert_temp(id, grab));
        }
        let grab = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(viewport * 0.5);
        let view = (beat - grab).clamp(0.0, (span - viewport).max(0.0));
        if view != arr.view_beats {
            arr.view_beats = view;
            moved = true;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    // --- paint -------------------------------------------------------------
    let painter = ui.painter();
    painter.rect_filled(strip, 0.0, theme.surface_sunken);
    if let Some((from, to)) = arr.loop_range {
        let band = egui::Rect::from_min_max(
            egui::pos2(strip.left() + from * scale, strip.top()),
            egui::pos2(strip.left() + to * scale, strip.bottom()),
        );
        painter.rect_filled(band.intersect(strip), 0.0, theme.loop_region);
    }
    let row_h = strip.height() / arr.tracks.len().max(1) as f32;
    let inset = (row_h * 0.2).clamp(0.5, 2.0);
    for (t, track) in arr.clips.iter().enumerate() {
        let top = strip.top() + t as f32 * row_h;
        for (i, clip) in track.iter().enumerate() {
            let x0 = strip.left() + clip.start * scale;
            // A short clip keeps one visible pixel: an empty row would read
            // as an empty lane.
            let x1 = (x0 + clip.len * scale).max(x0 + 1.0);
            let bar = egui::Rect::from_min_max(
                egui::pos2(x0, top + inset),
                egui::pos2(x1, top + row_h - inset),
            );
            let color = if arr.selected_clip == Some((t, i)) {
                theme.clip_selected
            } else {
                theme.clip_body
            };
            painter.rect_filled(bar.intersect(strip), 0.0, color);
        }
    }
    let x = strip.left() + playhead * scale;
    painter.line_segment(
        [egui::pos2(x, strip.top()), egui::pos2(x, strip.bottom())],
        egui::Stroke::new(1.0, theme.playhead),
    );

    // The view window, above everything it frames.
    let window = egui::Rect::from_min_max(
        egui::pos2(strip.left() + arr.view_beats * scale, strip.top()),
        egui::pos2(
            strip.left() + (arr.view_beats + viewport) * scale,
            strip.bottom(),
        ),
    )
    .intersect(strip);
    painter.rect_filled(window, 0.0, theme.accent_muted.gamma_multiply(0.35));
    painter.rect_stroke(
        window,
        0.0,
        egui::Stroke::new(
            1.0,
            if resp.hovered() || resp.is_pointer_button_down_on() {
                theme.accent
            } else {
                theme.outline
            },
        ),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [strip.left_bottom(), strip.right_bottom()],
        egui::Stroke::new(1.0, theme.divider),
    );
    moved
}

/// Preview an audio-file drag as a ghost clip: the lane under the pointer
/// washed as the target, the file at its true length for the session tempo,
/// snapped to the nearest grid line — and, below the last lane, the phantom
/// strip of the audio track a drop there would create. The ghost sits where
/// the clip will actually land (first-fit from the snapped beat), so it
/// never promises a spot the placement rules would refuse.
///
/// Returns where a release right now would land; None when it would not.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drop_preview(
    ui: &egui::Ui,
    theme: &Theme,
    content: egui::Rect,
    arr: &Arrangement,
    grid: f32,
    beats_per_bar: u32,
    bpm: f64,
    waveform_cache: &HashMap<PathBuf, Arc<waveform::Peaks>>,
    drag: &DragImport,
) -> Option<DropSpot> {
    let pos = ui.ctx().pointer_latest_pos()?;
    if !content.contains(pos) {
        return None;
    }
    let hint = |text: &str, color: egui::Color32| {
        ui.painter().text(
            pos + egui::vec2(14.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::new(11.0, egui::FontFamily::Proportional),
            color,
        );
    };
    if !drag.accepted {
        hint("WAV files only", theme.warn);
        return None;
    }

    let offset = arr.view_beats;
    let ppb = arr.pixels_per_beat;
    let lanes = lane_rects(content, &arr.tracks, arr.view_tracks_y);
    let below = lanes.last().map_or(content.top(), egui::Rect::bottom);
    let hovered = lanes
        .iter()
        .position(|lane| (lane.top()..lane.bottom()).contains(&pos.y));
    let (lane, track) = match hovered {
        Some(t) if arr.tracks[t].kind == TrackKind::Audio => (lanes[t], Some(t)),
        Some(_) => {
            hint("audio tracks only", theme.text_muted);
            return None;
        }
        // The empty space below the lanes, all of it: the phantom strip is
        // drawn where the new track will actually appear, not at the
        // pointer, so the preview and the outcome are the same picture.
        None => (
            egui::Rect::from_min_max(
                egui::pos2(content.left(), below),
                egui::pos2(content.right(), (below + TRACK_H).min(content.bottom())),
            ),
            None,
        ),
    };

    // The file's musical length — honest when the header could be read,
    // one bar as a stand-in when it could not.
    let len = drag
        .header
        .map(|(rate, frames)| (frames as f64 / f64::from(rate.max(1)) * bpm / 60.0) as f32)
        .filter(|len| len.is_finite() && *len > 0.0)
        .unwrap_or(beats_per_bar as f32);
    let at = snap(beat_at(content, offset, ppb, pos.x), grid);
    let start = match track {
        Some(t) => place_clip(&arr.clips[t], at, len).0,
        None => at,
    };
    let ghost = Clip {
        id: u64::MAX,
        name: drag.name.clone(),
        start,
        len,
        notes: Vec::new(),
        audio: drag.header.map(|(sample_rate, source_frames)| AudioSource {
            path: drag.path.clone(),
            sample_rate,
            source_offset: 0,
            source_frames,
            gain: 1.0,
            looped: false,
            file_frames: source_frames,
            reversed: false,
            fade_in: 0,
            fade_out: 0,
            fade_in_curve: 0.0,
            fade_out_curve: 0.0,
            envelope: Vec::new(),
        }),
        loop_on: false,
        loop_start: 0.0,
        loop_len: 0.0,
    };

    let painter = ui.painter();
    let visible_lane = lane.intersect(content);
    painter.rect_filled(
        visible_lane,
        0.0,
        egui::Color32::from_rgba_unmultiplied(
            theme.accent.r(),
            theme.accent.g(),
            theme.accent.b(),
            28,
        ),
    );
    if track.is_none() && visible_lane.height() > 0.0 {
        painter.rect_stroke(
            visible_lane,
            0.0,
            egui::Stroke::new(1.0, theme.divider),
            egui::StrokeKind::Inside,
        );
        if visible_lane.height() >= 20.0 {
            painter.text(
                egui::pos2(
                    visible_lane.right() - CLIP_LABEL_PAD,
                    visible_lane.top() + CLIP_LABEL_PAD,
                ),
                egui::Align2::RIGHT_TOP,
                "new audio track",
                egui::FontId::new(11.0, egui::FontFamily::Proportional),
                theme.text_muted,
            );
        }
    }

    let musical_r = clip_rect(content, offset, ppb, lane, &ghost);
    let visual = clip_canvas_rects(musical_r);
    let r = visual.outer.intersect(content);
    painter.rect_filled(r, 0.0, theme.clip_audio.gamma_multiply(0.72));
    painter.rect_filled(
        visual.title.intersect(content),
        0.0,
        theme.clip_audio_header.gamma_multiply(0.82),
    );
    if ghost.audio.is_some()
        && let Some(peaks) = waveform_cache.get(&drag.path)
    {
        waveform::paint_clip_thumbnail(
            ui,
            theme,
            waveform::ClipThumbnail {
                full_clip: visual.outer,
                visible_clip: r,
                clip: &ghost,
                peaks,
                bpm,
                opacity: 0.45,
            },
        );
    }
    let painter = ui.painter();
    painter.rect_stroke(
        r,
        0.0,
        egui::Stroke::new(stroke::BOLD, theme.clip_selected),
        egui::StrokeKind::Middle,
    );
    // The landing line: the full lane height at the landing beat, so the
    // grid position reads even when the ghost runs past the view.
    let x = x_at(content, offset, ppb, start);
    if x >= content.left() && x <= content.right() {
        painter.line_segment(
            [
                egui::pos2(x, visible_lane.top()),
                egui::pos2(x, visible_lane.bottom()),
            ],
            egui::Stroke::new(1.5, theme.accent),
        );
    }
    if r.width() >= CLIP_LABEL_MIN_W {
        let title = visual.title.intersect(content);
        painter.with_clip_rect(title).text(
            egui::pos2(title.left() + CLIP_LABEL_PAD, title.center().y),
            egui::Align2::LEFT_CENTER,
            &ghost.name,
            egui::FontId::new(font::LABEL, egui::FontFamily::Proportional),
            theme.text,
        );
    }
    Some(match track {
        Some(track) => DropSpot::Timeline { track, beat: at },
        None => DropSpot::NewTrack { beat: at },
    })
}

/// What the clip pass hands back to the app.
#[derive(Default)]
pub(crate) struct ClipsOutcome {
    /// A clip body was double-clicked; the lower region should show its
    /// editor.
    pub(crate) open_editor: bool,
    /// A fade handle was dragged. Carried out rather than written here,
    /// because a fade rides a LETTER to the clip's node — and letters are
    /// the app's to send, the same as the editor's own fade drag.
    pub(crate) fade: Option<(u64, waveform::ClipEdit)>,
}

/// What the clip context menu asked for.
#[derive(Clone, Copy)]
pub(crate) enum ClipMenu {
    Copy,
    Duplicate,
    Rename,
    Delete,
}

/// Draw and interact with the clips.
///
/// Runs AFTER the lane pass, so clips sit above the selection wash and their
/// interact rects are created after the lane bodies' — a clip wins the
/// pointer wherever they overlap, and a click on empty lane still starts a
/// time selection. Edits are collected while drawing and applied at the end:
/// the draw borrow stays read-only, and a drag reads last frame's rects
/// against this frame's delta — one frame of lag, invisible at 60fps.
// Clip drawing needs the panel geometry, timeline scale, model, and cache.
#[allow(clippy::too_many_arguments)]
pub(crate) fn clips_pass(
    ui: &mut egui::Ui,
    theme: &Theme,
    content: egui::Rect,
    arr: &mut Arrangement,
    grid: f32,
    bpm: f64,
    waveform_cache: &HashMap<PathBuf, Arc<waveform::Peaks>>,
    automation_mode: bool,
) -> ClipsOutcome {
    let offset = arr.view_beats;
    let pixels_per_beat = arr.pixels_per_beat;
    let lanes = lane_rects(content, &arr.tracks, arr.view_tracks_y);
    // The rename and the ghost ride out of `arr` for the draw — both are
    // frame-to-frame state the pass either finishes or hands back.
    let mut rename = arr.rename.take();
    let mut ghost = arr.ghost.take();

    // Command-click toggles membership; Shift-click extends from the primary;
    // ordinary clicks establish one decisive primary selection.
    let mut select: Option<(usize, u64, bool, bool)> = None;
    let mut left_to: Option<(usize, u64, f32)> = None;
    let mut right_to: Option<(usize, u64, f32)> = None;
    // The released ghost and the lane its ORIGINAL lives on — a move must
    // vacate that one, wherever the ghost itself has wandered.
    let mut ghost_finalize: Option<(Ghost, usize)> = None;
    // A Cell, because one context-menu closure is created per clip per
    // frame and they all need to reach the same slot.
    let menu: std::cell::Cell<Option<(usize, u64, ClipMenu)>> = std::cell::Cell::new(None);
    let mut rename_commit = false;
    let mut rename_cancel = false;
    let mut open_editor = false;
    let mut fade: Option<(u64, waveform::ClipEdit)> = None;
    // WHERE THE INSERT MARKER GOES when a clip is clicked.
    //
    // Empty lane ground has always moved it — "the press IS the insert
    // marker" — but a clip swallowed the press and left the transport
    // where it was. On a song whose tracks are covered in clips, which
    // is most songs, that made whole regions of the timeline impossible
    // to point at: you could not put the marker on beat one if a clip
    // started there.
    //
    // Collected rather than written, because the loop below holds `arr`
    // immutably while it draws.
    let mut seek_req: Option<f32> = None;

    for (t, track) in arr.clips.iter().enumerate() {
        let Some(full_lane) = lanes.get(t) else {
            break;
        };
        // A selected automated track gives the clips the upper portion of
        // its lane. The lower automation sublane is deliberately out of the
        // clip pass altogether, for both paint and hit testing.
        let lane = if automation_mode && arr.selected == Some(t) {
            egui::Rect::from_min_max(full_lane.min, automation_rect(*full_lane).min)
        } else {
            *full_lane
        };
        for (i, clip) in track.iter().enumerate() {
            let musical_rect = clip_rect(content, offset, pixels_per_beat, lane, clip);
            let visual = clip_canvas_rects(musical_rect);
            let rect = visual.outer.intersect(content);
            if rect.width() <= 0.0 {
                continue;
            }
            let selected = arr.clip_is_selected(t, i);
            let body_id = ui.id().with(("clip", clip.id));

            // Interact BEFORE painting: the strips, created after the body,
            // sit above it in hit-test order, and the paint lands on top of
            // both in the same order it is issued.
            let body = ui
                .interact(rect, body_id, egui::Sense::click_and_drag())
                .affords(Affords::Carry);
            let command = ui.input(|i| i.modifiers.command);
            let shift = ui.input(|i| i.modifiers.shift);
            if body.drag_started() {
                // Every drag rides a ghost. Plain drag moves the original
                // to wherever the ghost lands; Ctrl+drag leaves it and
                // lands a copy. The grip is measured from the PRESS origin:
                // drag_started only fires once the drag threshold is passed,
                // so the pointer has already left the grab point by then.
                let grab = ui.input(|i| i.pointer.press_origin()).map_or(0.0, |pos| {
                    beat_at(content, offset, pixels_per_beat, pos.x) - clip.start
                });
                // Everything else that is selected comes too — but
                // only if THIS clip is part of the selection. Dragging
                // an unselected clip is a fresh gesture about that clip
                // alone, and taking a stale selection with it would move
                // things the user had forgotten were chosen.
                let followers: Vec<(u64, isize)> = if selected {
                    arr.selected_clip_refs()
                        .into_iter()
                        .filter_map(|(ft, fi)| {
                            let other = arr.clips.get(ft)?.get(fi)?;
                            (other.id != clip.id).then_some((other.id, ft as isize - t as isize))
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                ghost = Some(Ghost {
                    track: t,
                    clip: clip.clone(),
                    grab,
                    copy: command,
                    from_start: clip.start,
                    followers,
                });
                if !selected {
                    select = Some((t, clip.id, false, false));
                }
            }
            if body.clicked() {
                select = Some((t, clip.id, command, shift));
                // A CLICK, and deliberately not a drag: dragging a clip
                // is moving it, and scrubbing the song along behind
                // every move would make the transport follow the mouse
                // whenever anyone rearranged anything. The lane pass
                // draws the same line for the same reason.
                if let Some(pos) = body.interact_pointer_pos() {
                    seek_req = Some(snap(beat_at(content, offset, pixels_per_beat, pos.x), grid));
                }
            }
            if body.secondary_clicked() && !selected {
                select = Some((t, clip.id, false, false));
            }
            if body.double_clicked() {
                select = Some((t, clip.id, false, false));
                open_editor = true;
            }
            if body.dragged()
                && let Some(g) = &mut ghost
                && g.clip.id == clip.id
                && let Some(pos) = body.interact_pointer_pos()
            {
                // Absolute from the pointer, not accumulated deltas: the
                // grip point stays under the finger, and snapping cannot
                // eat the motion a frame at a time.
                let want = beat_at(content, offset, pixels_per_beat, pos.x) - g.grab;
                g.clip.start = snap(want, grid);
                // Cross-lane: the ghost follows the pointer onto any lane
                // whose kind can hold the clip, and keeps its last lane
                // while the pointer is over one that cannot.
                if let Some(target) = lanes
                    .iter()
                    .position(|lane| (lane.top()..lane.bottom()).contains(&pos.y))
                    && lane_accepts(&arr.tracks[target], &g.clip)
                {
                    g.track = target;
                }
            }
            if body.drag_stopped() {
                // The ghost lands: place it wherever the first fitting gap
                // is, like any other new clip. Deferred to the apply
                // section — the draw loop still holds `arr.clips` borrowed.
                let active = ghost.as_ref().is_some_and(|g| g.clip.id == clip.id);
                if active {
                    ghost_finalize = ghost.take().map(|g| (g, t));
                }
            }
            if body.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if body.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }

            body.context_menu(|ui| {
                if ui.button("Copy").clicked() {
                    menu.set(Some((t, clip.id, ClipMenu::Copy)));
                    ui.close();
                }
                if ui.button("Duplicate").clicked() {
                    menu.set(Some((t, clip.id, ClipMenu::Duplicate)));
                    ui.close();
                }
                if ui.button("Rename").clicked() {
                    menu.set(Some((t, clip.id, ClipMenu::Rename)));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    menu.set(Some((t, clip.id, ClipMenu::Delete)));
                    ui.close();
                }
            });

            if selected {
                let left = egui::Rect::from_min_max(
                    rect.min,
                    egui::pos2((rect.left() + CLIP_EDGE_W).min(rect.right()), rect.bottom()),
                );
                let right = egui::Rect::from_min_max(
                    egui::pos2((rect.right() - CLIP_EDGE_W).max(rect.left()), rect.top()),
                    rect.max,
                );
                for (side, strip) in [(0usize, left), (1usize, right)] {
                    let wid = ui.id().with(("clip_edge", clip.id, side));
                    let resp = ui
                        .interact(strip, wid, egui::Sense::drag())
                        .affords(Affords::SeamX);
                    if resp.hovered() || resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if resp.dragged()
                        && let Some(pos) = resp.interact_pointer_pos()
                    {
                        let beat = beat_at(content, offset, pixels_per_beat, pos.x);
                        if side == 0 {
                            left_to = Some((t, clip.id, beat));
                        } else {
                            right_to = Some((t, clip.id, beat));
                        }
                    }
                }

                // The FADE handles, on audio clips only and AFTER the trim
                // strips so they win the pointer where the two meet — a
                // fade at zero sits exactly on the clip's edge, and the
                // top corner is the half of that edge Ableton gives to
                // fading rather than to trimming.
                if let Some(audio) = &clip.audio
                    && let Some(span) = waveform::fade_span_beats(clip, audio, bpm)
                {
                    for leading in [true, false] {
                        let at = if leading {
                            clip.start + span * audio.fade_in as f32
                        } else {
                            clip.start + clip.len - span * audio.fade_out as f32
                        };
                        let x = x_at(content, offset, pixels_per_beat, at);
                        if x < content.left() || x > content.right() {
                            continue;
                        }
                        let grip = egui::Rect::from_center_size(
                            egui::pos2(x, visual.title.center().y),
                            egui::vec2(CLIP_FADE_GRIP, visual.title.height()),
                        );
                        let wid = ui.id().with(("clip_fade", clip.id, leading));
                        let resp = ui
                            .interact(grip, wid, egui::Sense::drag())
                            .affords(Affords::Carry);
                        if resp.hovered() || resp.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                        }
                        if resp.dragged()
                            && let Some(pos) = resp.interact_pointer_pos()
                        {
                            let beat = beat_at(content, offset, pixels_per_beat, pos.x);
                            let from_edge = if leading {
                                beat - clip.start
                            } else {
                                clip.start + clip.len - beat
                            };
                            let frames = (from_edge.max(0.0) / span).round().max(0.0) as u64;
                            let frames = frames.min((clip.len / span).round().max(0.0) as u64);
                            fade = Some((
                                clip.id,
                                if leading {
                                    waveform::ClipEdit::FadeIn(frames)
                                } else {
                                    waveform::ClipEdit::FadeOut(frames)
                                },
                            ));
                            select = Some((t, clip.id, false, false));
                        }
                    }
                }
            }

            // --- paint ---------------------------------------------------
            // A clip whose move-ghost is in flight is lifted: it stays
            // visible where it was, dimmed, until the ghost lands.
            let lifted = ghost
                .as_ref()
                .is_some_and(|g| !g.copy && g.clip.id == clip.id);
            let painter = ui.painter();
            let audible = track_audible(&arr.tracks, t);
            let (body_colour, title_colour, type_name) = if clip.audio.is_some() {
                (theme.clip_audio, theme.clip_audio_header, "AUDIO")
            } else {
                (theme.clip_midi, theme.clip_midi_header, "MIDI")
            };
            // TWO STATES, TWO CHANNELS.
            //
            // These both used to ride opacity — mid-drag at 0.35, muted
            // at 0.5, audible at 1.0 — so three states sat on one axis
            // separable only by degree, and "is that clip muted or just
            // the one I am dragging?" was answerable by remembering
            // which number was which.
            //
            // Opacity now means IN FLIGHT and nothing else: a clip being
            // carried is a ghost of itself, which is what a proposal
            // looks like. Muted is a hatch — present, and not in the
            // path — which is the same mark the mixer's refused route
            // wears, for the same reason. See `ui::hud`.
            let strength = if lifted { 0.35 } else { 1.0 };
            painter.rect_filled(rect, 0.0, body_colour.gamma_multiply(strength));
            let title_visible = visual.title.intersect(content);
            painter.rect_filled(title_visible, 0.0, title_colour.gamma_multiply(strength));
            if !audible {
                daw::ui::hud::hatch(
                    &painter.with_clip_rect(rect),
                    rect,
                    egui::Stroke::new(stroke::HAIR, theme.bg.gamma_multiply(0.7 * strength)),
                );
            }

            // MORE OF THE FILE PAST THIS EDGE.
            //
            // A clip trimmed to a quarter of its file and one that IS
            // its file were drawn identically, so "can I pull this edge
            // out further" was a question you answered by trying. A
            // notch cut out of the corner says there is more where that
            // came from — on the side it is on, which is the half a
            // single marker could not say.
            //
            // Cut INTO the clip rather than drawn beside it: the mark
            // belongs to the edge it describes, and a triangle sitting
            // outside would read as something in the lane.
            if let Some(audio) = &clip.audio {
                let (before, after) = audio.spare();
                let notch = (rect.height() * 0.28).min(NOTCH_MAX);
                let ink = theme.bg.gamma_multiply(0.85 * strength);
                if rect.width() > notch * 2.0 {
                    if before {
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                rect.left_bottom(),
                                egui::pos2(rect.left() + notch, rect.bottom()),
                                egui::pos2(rect.left(), rect.bottom() - notch),
                            ],
                            ink,
                            egui::Stroke::NONE,
                        ));
                    }
                    if after {
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                rect.right_bottom(),
                                egui::pos2(rect.right() - notch, rect.bottom()),
                                egui::pos2(rect.right(), rect.bottom() - notch),
                            ],
                            ink,
                            egui::Stroke::NONE,
                        ));
                    }
                }
            }
            if title_visible.height() > 2.0 {
                painter.line_segment(
                    [title_visible.left_bottom(), title_visible.right_bottom()],
                    egui::Stroke::new(stroke::HAIR, theme.divider.gamma_multiply(strength)),
                );
            }
            if let Some(audio) = &clip.audio
                && let Some(peaks) = waveform_cache.get(&audio.path)
            {
                waveform::paint_clip_thumbnail(
                    ui,
                    theme,
                    waveform::ClipThumbnail {
                        full_clip: visual.outer,
                        visible_clip: rect,
                        clip,
                        peaks,
                        bpm,
                        opacity: if lifted {
                            0.25
                        } else if selected {
                            0.95
                        } else if !audible {
                            0.35
                        } else {
                            0.72
                        },
                    },
                );
            }
            // WHERE THIS CLIP BEGINS, always.
            //
            // A clip's boundary carried no ink at all unless it happened
            // to be selected or under the pointer — so two clips sharing
            // an edge, which is exactly what splitting one produces, drew
            // as a single block. The one operation whose whole purpose is
            // to make two things out of one left no evidence it had run.
            //
            // The LEADING edge, not both: an object needs a mark where it
            // starts, and marking both ends would draw every boundary
            // twice wherever clips abut — which is most of a finished
            // arrangement. In the clip's own header colour, so the line
            // reads as belonging to the clip that starts there rather
            // than as a rule in the lane.
            let start_x = rect.left();
            if start_x >= content.left() && start_x <= content.right() {
                painter.line_segment(
                    [
                        egui::pos2(start_x, rect.top()),
                        egui::pos2(start_x, rect.bottom()),
                    ],
                    egui::Stroke::new(stroke::HAIR, title_colour.gamma_multiply(strength)),
                );
            }

            if selected || body.hovered() {
                painter.rect_stroke(
                    rect,
                    0.0,
                    egui::Stroke::new(
                        if selected {
                            stroke::FOCUS
                        } else {
                            stroke::HAIR
                        },
                        if selected {
                            theme.clip_selected
                        } else {
                            theme.clip_hover
                        },
                    ),
                    egui::StrokeKind::Middle,
                );
            }
            if selected {
                // The invisible edge strips get small physical grips once
                // selected, so resize is discoverable without permanent
                // handles cluttering every clip.
                painter.rect_filled(
                    visual.left_grip.intersect(content),
                    0.0,
                    theme.clip_selected,
                );
                painter.rect_filled(
                    visual.right_grip.intersect(content),
                    0.0,
                    theme.clip_selected,
                );
            }

            if let Some(audio) = &clip.audio
                && let Some(span) = waveform::fade_span_beats(clip, audio, bpm)
                && (audio.fade_in > 0 || audio.fade_out > 0)
            {
                let body = visual.outer.intersect(content);
                let ink = theme.clip_selected.gamma_multiply(0.9);
                if audio.fade_in > 0 {
                    let to = x_at(
                        content,
                        offset,
                        pixels_per_beat,
                        clip.start + span * audio.fade_in as f32,
                    );
                    painter.line_segment(
                        [
                            egui::pos2(body.left(), body.bottom()),
                            egui::pos2(to.min(body.right()), body.top()),
                        ],
                        egui::Stroke::new(stroke::HAIR, ink),
                    );
                }
                if audio.fade_out > 0 {
                    let from = x_at(
                        content,
                        offset,
                        pixels_per_beat,
                        clip.start + clip.len - span * audio.fade_out as f32,
                    );
                    painter.line_segment(
                        [
                            egui::pos2(from.max(body.left()), body.top()),
                            egui::pos2(body.right(), body.bottom()),
                        ],
                        egui::Stroke::new(stroke::HAIR, ink),
                    );
                }
            }

            if !clip.notes.is_empty() {
                let pitch_lo = clip.notes.iter().map(|n| n.pitch).min().unwrap_or(0);
                let pitch_hi = clip.notes.iter().map(|n| n.pitch).max().unwrap_or(0);
                // The notes AS THEY SOUND, brace and all — the same
                // unroll the engine compiles. Drawing the stored notes
                // instead showed a looping clip playing its pattern once
                // and then apparently nothing, which is a picture of a
                // different clip from the one you can hear.
                //
                // Borrowed where the clip does not loop, so the common
                // case still costs no allocation.
                let sounding: std::borrow::Cow<'_, [Note]> = if clip.loop_on {
                    std::borrow::Cow::Owned(clip_notes(clip, clip.len))
                } else {
                    std::borrow::Cow::Borrowed(&clip.notes)
                };
                for note in sounding.iter() {
                    // Map time against the WHOLE clip, then crop it to the
                    // visible fragment. Mapping against `rect` would
                    // stretch every note across whatever remained after
                    // follow/panning moved part of the clip off screen.
                    let r = note_rect(visual.content, note, pitch_lo, pitch_hi, clip.len)
                        .intersect(rect);
                    if r.width() > 0.0 && r.height() > 0.0 {
                        let colour =
                            theme
                                .clip_note
                                .gamma_multiply(if audible { 0.9 } else { 0.4 });
                        if note.muted {
                            painter.rect_stroke(
                                r,
                                0.0,
                                egui::Stroke::new(stroke::HAIR, colour.gamma_multiply(0.55)),
                                egui::StrokeKind::Inside,
                            );
                        } else {
                            painter.rect_filled(r, 0.0, colour);
                        }
                    }
                }
            }

            if let Some(r) = rename.as_mut().filter(|r| r.track == t && r.id == clip.id) {
                // The name label becomes the edit box, in place.
                let edit = egui::Rect::from_min_size(
                    egui::pos2(
                        title_visible.left() + CLIP_LABEL_PAD,
                        title_visible.top() + 1.0,
                    ),
                    egui::vec2(
                        (title_visible.width() - 2.0 * CLIP_LABEL_PAD).max(20.0),
                        (title_visible.height() - 2.0).max(1.0),
                    ),
                );
                let resp = ui.put(edit, egui::TextEdit::singleline(&mut r.text));
                if !r.focused {
                    resp.request_focus();
                    r.focused = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    rename_cancel = true;
                } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) || resp.lost_focus() {
                    rename_commit = true;
                }
            } else if title_visible.width() >= CLIP_LABEL_MIN_W {
                // Clipped to the clip: a name longer than the box it names
                // is cut off at the edge rather than running across its
                // neighbours.
                let metadata_w = if visual.outer.width() >= CLIP_TYPE_MIN_W {
                    36.0
                } else {
                    0.0
                };
                let name_room = egui::Rect::from_min_max(
                    title_visible.min,
                    egui::pos2(
                        (title_visible.right() - metadata_w).max(title_visible.left()),
                        title_visible.bottom(),
                    ),
                );
                painter.with_clip_rect(name_room).text(
                    egui::pos2(
                        title_visible.left() + CLIP_LABEL_PAD,
                        title_visible.center().y,
                    ),
                    egui::Align2::LEFT_CENTER,
                    &clip.name,
                    egui::FontId::new(font::LABEL, egui::FontFamily::Proportional),
                    if audible {
                        theme.text
                    } else {
                        theme.text_muted
                    },
                );
                if metadata_w > 0.0 {
                    painter.with_clip_rect(title_visible).text(
                        egui::pos2(
                            title_visible.right() - CLIP_LABEL_PAD,
                            title_visible.center().y,
                        ),
                        egui::Align2::RIGHT_CENTER,
                        type_name,
                        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
                        theme.text_muted,
                    );
                }
            }
        }
    }

    // An in-flight ghost is cancelled by Escape: the drag goes dead and the
    // original stays exactly where it was.
    if ghost.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        ghost = None;
    }

    // An incompatible lane is marked where the pointer actually is. The
    // ghost deliberately remains on its last valid lane, while this local
    // refusal explains why it did not follow.
    if let Some(g) = &ghost
        && let Some(pos) = ui.ctx().pointer_latest_pos()
        && let Some((target, lane)) = lanes
            .iter()
            .enumerate()
            .find(|(_, lane)| (lane.top()..lane.bottom()).contains(&pos.y))
        && !lane_accepts(&arr.tracks[target], &g.clip)
    {
        let visible = lane.intersect(content);
        ui.painter().rect_filled(
            visible,
            0.0,
            egui::Color32::from_rgba_unmultiplied(
                theme.danger.r(),
                theme.danger.g(),
                theme.danger.b(),
                if theme.light { 24 } else { 34 },
            ),
        );
        ui.painter().line_segment(
            [visible.left_top(), visible.right_top()],
            egui::Stroke::new(stroke::BOLD, theme.danger),
        );
        ui.painter().text(
            pos + egui::vec2(space::SM, -space::SM),
            egui::Align2::LEFT_BOTTOM,
            if g.clip.audio.is_some() {
                "AUDIO TRACK ONLY"
            } else {
                "MIDI TRACK ONLY"
            },
            egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
            theme.danger,
        );
    }

    // --- the ghost, drawn above everything it might land on ---------------
    if let Some(g) = &ghost
        && let Some(full_lane) = lanes.get(g.track)
    {
        let lane = if automation_mode && arr.selected == Some(g.track) {
            egui::Rect::from_min_max(full_lane.min, automation_rect(*full_lane).min)
        } else {
            *full_lane
        };
        // The target lane washed like a drop target, so a cross-lane drag
        // says where it is aimed even before the ghost is looked at.
        ui.painter().rect_filled(
            lane.intersect(content),
            0.0,
            egui::Color32::from_rgba_unmultiplied(
                theme.accent.r(),
                theme.accent.g(),
                theme.accent.b(),
                24,
            ),
        );
        let musical_r = clip_rect(content, offset, pixels_per_beat, lane, &g.clip);
        let visual = clip_canvas_rects(musical_r);
        let r = visual.outer.intersect(content);
        let (body_colour, title_colour) = if g.clip.audio.is_some() {
            (theme.clip_audio, theme.clip_audio_header)
        } else {
            (theme.clip_midi, theme.clip_midi_header)
        };
        let painter = ui.painter();
        painter.rect_filled(r, 0.0, body_colour.gamma_multiply(0.7));
        painter.rect_filled(
            visual.title.intersect(content),
            0.0,
            title_colour.gamma_multiply(0.8),
        );
        if let Some(audio) = &g.clip.audio
            && let Some(peaks) = waveform_cache.get(&audio.path)
        {
            waveform::paint_clip_thumbnail(
                ui,
                theme,
                waveform::ClipThumbnail {
                    full_clip: visual.outer,
                    visible_clip: r,
                    clip: &g.clip,
                    peaks,
                    bpm,
                    opacity: 0.45,
                },
            );
        }
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(stroke::BOLD, theme.clip_selected),
            egui::StrokeKind::Middle,
        );
        // A full-height snap guide ties the floating object to its exact
        // landing beat before release.
        let landing_x = x_at(content, offset, pixels_per_beat, g.clip.start);
        if content.left() <= landing_x && landing_x <= content.right() {
            painter.line_segment(
                [
                    egui::pos2(landing_x, lane.top().max(content.top())),
                    egui::pos2(landing_x, lane.bottom().min(content.bottom())),
                ],
                egui::Stroke::new(stroke::FOCUS, theme.accent),
            );
        }
        if r.width() >= CLIP_LABEL_MIN_W {
            let title = visual.title.intersect(content);
            painter.with_clip_rect(title).text(
                egui::pos2(title.left() + CLIP_LABEL_PAD, title.center().y),
                egui::Align2::LEFT_CENTER,
                &g.clip.name,
                egui::FontId::new(font::LABEL, egui::FontFamily::Proportional),
                theme.text,
            );
        }
    }

    // --- apply, after the draw borrow is done -----------------------------
    let index_of = |track: &[Clip], id: u64| track.iter().position(|c| c.id == id);

    if let Some((t, id, act)) = menu.get() {
        match act {
            ClipMenu::Copy => {
                if let Some(i) = index_of(&arr.clips[t], id) {
                    if !arr.clip_is_selected(t, i) {
                        arr.select_only_clip(t, i);
                    }
                    arr.copy_selected();
                }
            }
            ClipMenu::Duplicate => {
                if let Some(i) = index_of(&arr.clips[t], id) {
                    if !arr.clip_is_selected(t, i) {
                        arr.select_only_clip(t, i);
                    }
                    arr.duplicate_selected();
                }
            }
            ClipMenu::Rename => {
                if let Some(i) = index_of(&arr.clips[t], id) {
                    arr.select_only_clip(t, i);
                    rename = Some(Rename {
                        track: t,
                        id,
                        text: arr.clips[t][i].name.clone(),
                        original: arr.clips[t][i].name.clone(),
                        focused: false,
                    });
                }
            }
            ClipMenu::Delete => {
                if let Some(i) = index_of(&arr.clips[t], id) {
                    if arr.clip_is_selected(t, i) {
                        arr.delete_selected_clips();
                    } else {
                        arr.remove_clip(t, id);
                    }
                }
            }
        }
    }

    // Rename settlement: Escape restores the original, Enter or clicking
    // away keeps the edit — but never an empty name, whatever the key was.
    if rename_cancel {
        if let Some(r) = &rename
            && let Some(c) = arr.clips[r.track].iter_mut().find(|c| c.id == r.id)
        {
            c.name = r.original.clone();
        }
        rename = None;
    } else if rename_commit {
        if rename.as_ref().is_some_and(|r| r.text.trim().is_empty())
            && let Some(r) = &rename
            && let Some(c) = arr.clips[r.track].iter_mut().find(|c| c.id == r.id)
        {
            c.name = r.original.clone();
        }
        rename = None;
    } else if let Some(r) = &rename
        && let Some(c) = arr.clips[r.track].iter_mut().find(|c| c.id == r.id)
    {
        // Live: the clip reads its new name while it is being typed.
        c.name = r.text.clone();
    }
    arr.rename = rename;

    // The clips pass runs after the lanes', so this wins over a marker
    // the lane below the clip might have asked for.
    if let Some(beat) = seek_req {
        arr.pending_point = Some(beat);
    }
    if let Some((t, id, additive, extend)) = select
        && let Some(i) = index_of(&arr.clips[t], id)
    {
        if additive {
            arr.toggle_clip_selection(t, i);
        } else if extend {
            arr.extend_clip_selection(t, i);
        } else {
            arr.select_only_clip(t, i);
        }
    }
    if let Some((t, id, want)) = left_to
        && let Some(i) = index_of(&arr.clips[t], id)
    {
        let old_start = arr.clips[t][i].start;
        let mut want = snap(want, grid);
        if let Some(audio) = &arr.clips[t][i].audio {
            let available_beats =
                audio.source_offset as f64 / f64::from(audio.sample_rate) * bpm.max(1.0) / 60.0;
            want = want.max(old_start - available_beats as f32);
        }
        let start = clamp_clip_start(&arr.clips[t], i, want);
        trim_clip_left(&mut arr.clips[t][i], start, bpm);
    }
    if let Some((t, id, want)) = right_to
        && let Some(i) = index_of(&arr.clips[t], id)
    {
        // `want` is the BEAT UNDER THE POINTER; `len` is a length. The
        // clip's start has to come off, and it did not — so the new
        // length was the absolute end position and every clip grew by
        // exactly its own start beat. A clip at bar three dragged to
        // bar four became seven bars long.
        //
        // It survived because a clip at beat zero gets the right answer
        // by coincidence, which is what most fixtures are.
        //
        // The END is snapped, not the length: a clip that begins off the
        // grid should still be draggable to a grid line, and snapping
        // its length instead would carry the offset into every edge it
        // ever has.
        let mut len = drag_len(want, arr.clips[t][i].start, grid);
        if let Some(audio) = &arr.clips[t][i].audio
            && !audio.looped
        {
            let source_beats =
                audio.source_frames as f64 / f64::from(audio.sample_rate) * bpm.max(1.0) / 60.0;
            len = len.min(source_beats as f32);
        }
        arr.clips[t][i].len = clamp_clip_len(&arr.clips[t], i, len, grid);
    }

    // The released ghost becomes a real clip: first fitting gap, selected.
    // A move vacates its original FIRST, so the landing can take the very
    // spot the original held; a copy keeps it and lands under a fresh id —
    // ids are identity, and identity is never in two places.
    if let Some((g, src)) = ghost_finalize {
        arr.land_ghost(g, src);
    }
    arr.ghost = ghost;
    ClipsOutcome { open_editor, fade }
}

/// The locators: flags in the ruler. Click a flag to jump the playhead to
/// it, drag to move it (snapped), double-click to rename in place, and the
/// context menu renames or deletes. Created AFTER the loop brace, so a
/// flag wins the pointer where the two overlap.
pub(crate) fn locators_pass(
    ui: &mut egui::Ui,
    theme: &Theme,
    ruler: egui::Rect,
    content: egui::Rect,
    arr: &mut Arrangement,
    grid: f32,
) {
    let offset = arr.view_beats;
    let ppb = arr.pixels_per_beat;
    let mut rename = arr.locator_rename.take();
    let mut jump: Option<f32> = None;
    let mut move_to: Option<(usize, f32)> = None;
    let mut rename_open: Option<usize> = None;
    let delete: std::cell::Cell<Option<usize>> = std::cell::Cell::new(None);
    let mut rename_commit = false;
    let mut rename_cancel = false;

    for (index, locator) in arr.locators.iter().enumerate() {
        let x = x_at(content, offset, ppb, locator.beat);
        if x < content.left() - 8.0 || x > content.right() + 8.0 {
            continue;
        }
        let flag = egui::Rect::from_min_max(
            egui::pos2(x - 4.0, ruler.top()),
            egui::pos2(x + 5.0, ruler.bottom()),
        );
        let wid = ui.id().with(("locator", index));
        let response = ui
            .interact(flag, wid, egui::Sense::click_and_drag())
            .affords(Affords::Carry);
        if response.double_clicked() {
            rename_open = Some(index);
        } else if response.clicked() {
            jump = Some(locator.beat);
        }
        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            move_to = Some((index, snap(beat_at(content, offset, ppb, pos.x), grid)));
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        response.context_menu(|ui| {
            if ui.button("Rename").clicked() {
                delete.set(None);
                rename_open = Some(index);
                ui.close();
            }
            if ui.button("Delete").clicked() {
                delete.set(Some(index));
                ui.close();
            }
        });

        let painter = ui.painter();
        let color = if response.hovered() || response.dragged() {
            theme.accent
        } else {
            theme.text_muted
        };
        // The flag: a stem on the beat, a pennant to the right.
        painter.line_segment(
            [
                egui::pos2(x, ruler.top() + 1.0),
                egui::pos2(x, ruler.bottom()),
            ],
            egui::Stroke::new(1.5, color),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x, ruler.top() + 1.0),
                egui::pos2(x + 7.0, ruler.top() + 4.5),
                egui::pos2(x, ruler.top() + 8.0),
            ],
            color,
            egui::Stroke::NONE,
        ));
        if rename.as_ref().is_some_and(|r| r.index == index) {
            if let Some(r) = rename.as_mut() {
                let edit = egui::Rect::from_min_size(
                    egui::pos2(x + 9.0, ruler.top()),
                    egui::vec2(90.0, ruler.height()),
                );
                let field = ui.put(
                    edit,
                    egui::TextEdit::singleline(&mut r.text)
                        .font(egui::FontId::proportional(HEADER_KIND_TYPE)),
                );
                if !r.focused {
                    field.request_focus();
                    r.focused = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    rename_cancel = true;
                } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) || field.lost_focus() {
                    rename_commit = true;
                }
            }
        } else {
            painter.with_clip_rect(ruler).text(
                egui::pos2(x + 9.0, ruler.center().y),
                egui::Align2::LEFT_CENTER,
                &locator.name,
                egui::FontId::proportional(HEADER_KIND_TYPE),
                color,
            );
        }
    }

    // --- apply -------------------------------------------------------------
    if rename_cancel {
        rename = None;
    } else if rename_commit
        && let Some(r) = rename.take()
        && let Some(locator) = arr.locators.get_mut(r.index)
    {
        let text = r.text.trim();
        locator.name = if text.is_empty() {
            r.original
        } else {
            text.to_owned()
        };
    }
    if let Some(index) = rename_open
        && let Some(locator) = arr.locators.get(index)
    {
        rename = Some(LocatorRename {
            index,
            text: locator.name.clone(),
            original: locator.name.clone(),
            focused: false,
        });
    }
    arr.locator_rename = rename;
    if let Some((index, beat)) = move_to
        && let Some(locator) = arr.locators.get_mut(index)
    {
        locator.beat = beat;
    }
    if let Some(index) = delete.get()
        && index < arr.locators.len()
    {
        arr.locators.remove(index);
        arr.locator_rename = None;
    }
    if let Some(beat) = jump {
        arr.pending_seek = Some(beat);
    }
}

/// The loop brace and its two handles, in the ruler strip.
///
/// Each end drags independently and snaps to the grid. They cannot be pulled
/// through each other: crossing would silently invert the loop, so the
/// dragged end stops one grid unit short of the other. The brace BODY drags
/// the whole loop — both ends at once, length untouched, snapped, and never
/// before beat 0.
pub(crate) fn loop_brace(
    ui: &mut egui::Ui,
    theme: &Theme,
    focus: &mut Focus,
    ruler: egui::Rect,
    content: egui::Rect,
    arr: &mut Arrangement,
    grid: f32,
) {
    ui.painter().line_segment(
        [
            egui::pos2(ruler.left(), ruler.bottom()),
            egui::pos2(ruler.right(), ruler.bottom()),
        ],
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );

    let Some((from, to)) = arr.loop_range else {
        return;
    };
    let offset = arr.view_beats;
    let pixels_per_beat = arr.pixels_per_beat;
    let (x0, x1) = (
        x_at(content, offset, pixels_per_beat, from),
        x_at(content, offset, pixels_per_beat, to),
    );
    let brace = egui::Rect::from_min_max(
        egui::pos2(x0, ruler.top() + 2.0),
        egui::pos2(x1, ruler.bottom() - 2.0),
    );

    // The brace body carries the WHOLE loop. Created before the handles, so
    // the handles win the pointer wherever they overlap the body.
    let body_id = ui.id().with("loop_body");
    let body = ui
        .interact(brace, body_id, egui::Sense::drag())
        .affords(Affords::Carry);
    if body.hovered() || body.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    let mut next = (from, to);
    if body.drag_started() {
        // The loop's start at press, kept for the drag's lifetime: the body
        // drag is absolute from here. Snapping a per-frame delta would eat
        // slow motion a frame at a time and the brace would stick.
        ui.data_mut(|d| d.insert_temp(body_id, from));
    }
    if body.dragged()
        && let Some(pos) = body.interact_pointer_pos()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && let Some(from0) = ui.data(|d| d.get_temp::<f32>(body_id))
    {
        let want = from0 + (pos.x - origin.x) / pixels_per_beat;
        next = loop_move((from, to), snap(want, grid) - from);
    }
    let (hx0, hx1) = (
        x_at(content, offset, pixels_per_beat, next.0),
        x_at(content, offset, pixels_per_beat, next.1),
    );
    for (which, x) in [(0usize, hx0), (1usize, hx1)] {
        let handle = egui::Rect::from_min_max(
            egui::pos2(x - LOOP_HANDLE_W * 0.5, ruler.top()),
            egui::pos2(x + LOOP_HANDLE_W * 0.5, ruler.bottom()),
        );
        let wid = ui.id().with(("loop_handle", which));
        focus.register(wid, handle);
        let response = ui
            .interact(handle, wid, egui::Sense::drag())
            .affords(Affords::SeamX);
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if let Some(pos) = response.interact_pointer_pos()
            && response.dragged()
        {
            let at = snap(beat_at(content, offset, pixels_per_beat, pos.x), grid);
            if which == 0 {
                next.0 = at.min(next.1 - grid).max(0.0);
            } else {
                next.1 = at.max(next.0 + grid);
            }
        }
    }
    arr.loop_range = Some(next);

    // Paint the brace LAST, at its final position for this frame — so both
    // handle drags and body drags feed back live instead of one frame late.
    let (nx0, nx1) = (
        x_at(content, offset, pixels_per_beat, next.0),
        x_at(content, offset, pixels_per_beat, next.1),
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(nx0, ruler.top() + 2.0),
            egui::pos2(nx1, ruler.bottom() - 2.0),
        )
        .intersect(ruler),
        0.0,
        theme.loop_brace,
    );
}
