use super::state::{ArrangementState, TrackRename};
use super::{Outcome, View};
use super::{edit, palette};
use crate::sequencing::{BlockId, Song, TICKS_PER_BEAT, TrackKind};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::redesign::grammar::{Motion, Utterance, Voice};
use crate::ui::redesign::registers::{ClipPayload, Payload, Registers};
use crate::ui::redesign::verbs::Verb;
use crate::ui::redesign::{OUTLINE, SURFACE_FRAME, focus};
use crate::ui::tokens::{font, space, stroke};
use eframe::egui;

const RULER_H: f32 = 36.0;
const TRACK_H: f32 = 64.0;
const TRACK_HEADER_W: f32 = 156.0;
const SONG_BARS: usize = 16;
const BEATS_PER_BAR: usize = 4;
const SONG_BEATS: usize = SONG_BARS * BEATS_PER_BAR;

const CANVAS: egui::Color32 = egui::Color32::BLACK;
const RULER: egui::Color32 = egui::Color32::from_gray(10);
const TRACK_HEADER: egui::Color32 = egui::Color32::from_gray(14);
const TRACK_LANE: egui::Color32 = egui::Color32::from_gray(6);
const BLOCK: egui::Color32 = egui::Color32::from_gray(20);
const QUIET: egui::Color32 = egui::Color32::from_gray(42);
const SILENT: egui::Color32 = egui::Color32::from_gray(68);
const MUTED: egui::Color32 = egui::Color32::from_gray(112);
const ACTIVE: egui::Color32 = egui::Color32::from_gray(196);

enum PointerEdit {
    Create,
    Move {
        id: BlockId,
        track: usize,
        start_tick: usize,
        copy: bool,
    },
    Resize {
        id: BlockId,
        start_tick: usize,
        length_ticks: usize,
    },
}

pub(super) fn show(
    ui: &mut egui::Ui,
    focused: bool,
    voice: &mut Voice<'_>,
    state: &mut ArrangementState,
    input: View<'_>,
) -> Outcome {
    let area = ui.available_rect_before_wrap();
    ui.take_available_space();
    let mut outcome = Outcome::default();
    ui.painter().rect_filled(area, 0.0, CANVAS);
    if area.width() < TRACK_HEADER_W + 80.0 || area.height() < RULER_H + TRACK_H {
        return outcome;
    }
    let mut command = None;
    // A rename in progress is TYPING mode: the text field owns the
    // keyboard, so the grammar stays silent until it closes.
    if focused && !state.palette.open && state.rename.is_none() {
        keyboard(ui, voice, state, input.song, &mut outcome);
    }
    if focused && state.palette.open {
        let selection = state.selection();
        command = palette::show(ui.ctx(), &mut state.palette, input.song, selection);
    }
    if let Some(command) = command {
        state.notice = Some(edit::apply(command, input.song, state.selection()));
        if command == edit::Command::DeleteClips {
            state.selected_block = None;
        }
        // Closure: the result stays addressed. A new lane takes the
        // cursor; a moved lane carries it along.
        match command {
            edit::Command::AddTrack => {
                state.cursor.track = input.song.tracks.len().saturating_sub(1);
            }
            edit::Command::MoveTrackUp => {
                state.cursor.track = state.cursor.track.saturating_sub(1);
            }
            edit::Command::MoveTrackDown => {
                state.cursor.track =
                    (state.cursor.track + 1).min(input.song.tracks.len().saturating_sub(1));
            }
            _ => {}
        }
    }

    let ruler = egui::Rect::from_min_max(area.min, egui::pos2(area.right(), area.top() + RULER_H));
    let timeline = egui::Rect::from_min_max(
        egui::pos2(area.left() + TRACK_HEADER_W, ruler.bottom()),
        area.right_bottom(),
    );
    draw_ruler(ui, ruler, timeline, state.view_start);
    let mut pointer_edit = None;
    let mut rename_outcome = RenameOutcome::Editing;

    let any_solo = input.song.tracks.iter().any(|track| track.solo);
    for (track_index, track) in input.song.tracks.iter().enumerate() {
        let top = timeline.top() + track_index as f32 * TRACK_H;
        if top >= timeline.bottom() {
            break;
        }
        let bottom = (top + TRACK_H).min(timeline.bottom());
        let header = egui::Rect::from_min_max(
            egui::pos2(area.left(), top),
            egui::pos2(timeline.left(), bottom),
        );
        let lane = egui::Rect::from_min_max(
            egui::pos2(timeline.left(), top),
            egui::pos2(timeline.right(), bottom),
        );
        draw_track(
            ui,
            header,
            lane,
            track_index,
            &track.name,
            &track.kind,
            track.muted,
            track.solo,
            any_solo,
        );
        if let Some(rename) = &mut state.rename
            && rename.track == track_index
        {
            rename_outcome = rename_editor(ui, header, rename);
        }

        let lane_response = ui
            .interact(
                lane,
                ui.id().with(("arrangement-lane", track.id.0)),
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::Draw);
        if let Some(pointer) = lane_response.interact_pointer_pos() {
            let beat = x_beat(lane, pointer.x, state.view_start);
            let block = track.blocks.iter().find(|block| {
                let start = block.start_tick / TICKS_PER_BEAT;
                let end = block
                    .start_tick
                    .saturating_add(block.length_ticks)
                    .div_ceil(TICKS_PER_BEAT);
                start <= beat && beat < end
            });
            if lane_response.drag_started() {
                if let Some(block) = block {
                    state.select_block(
                        block.id,
                        track_index,
                        block.start_tick / TICKS_PER_BEAT,
                        block
                            .start_tick
                            .saturating_add(block.length_ticks)
                            .div_ceil(TICKS_PER_BEAT),
                    );
                    let raw_beat = x_raw_beat(lane, pointer.x, state.view_start);
                    state.block_drag = Some(super::state::BlockDrag {
                        id: block.id,
                        grab_beats: raw_beat - block.start_tick as f32 / TICKS_PER_BEAT as f32,
                        copy: ui.input(|input| input.modifiers.command),
                        fine: ui.input(|input| input.modifiers.alt),
                    });
                } else {
                    state.begin_marquee(track_index, beat);
                }
                outcome.claim_focus = true;
            } else if lane_response.dragged() {
                if state.block_drag.is_none() {
                    let target_track = y_track(timeline, pointer.y, input.song.tracks.len());
                    state.drag_marquee(target_track, beat);
                }
            } else if lane_response.drag_stopped() {
                if let Some(drag) = state.block_drag {
                    let target_track = y_track(timeline, pointer.y, input.song.tracks.len());
                    let raw =
                        (x_raw_beat(lane, pointer.x, state.view_start) - drag.grab_beats).max(0.0);
                    let at = if drag.fine { raw } else { raw.round() };
                    pointer_edit = Some(PointerEdit::Move {
                        id: drag.id,
                        track: target_track,
                        start_tick: (at * TICKS_PER_BEAT as f32).round() as usize,
                        copy: drag.copy,
                    });
                }
                state.end_pointer_gesture();
            } else if lane_response.clicked() {
                if ui.input(|input| input.modifiers.shift) {
                    state.extend_to(track_index, beat);
                } else if let Some(block) = block {
                    state.select_block(
                        block.id,
                        track_index,
                        block.start_tick / TICKS_PER_BEAT,
                        block
                            .start_tick
                            .saturating_add(block.length_ticks)
                            .div_ceil(TICKS_PER_BEAT),
                    );
                    if lane_response.double_clicked() {
                        outcome.open_pattern = true;
                    }
                } else if lane_response.double_clicked() {
                    state.select_region(track_index, beat, beat + BEATS_PER_BAR);
                    pointer_edit = Some(PointerEdit::Create);
                } else {
                    state.select_region(track_index, beat, beat + 1);
                }
                outcome.claim_focus = true;
            }
        }
        let menu_beat = lane_response
            .hover_pos()
            .map_or(state.cursor.beat, |pointer| {
                x_beat(lane, pointer.x, state.view_start)
            });
        lane_response.context_menu(|ui| {
            if ui.button("New clip").clicked() {
                state.select_region(track_index, menu_beat, menu_beat + BEATS_PER_BAR);
                pointer_edit = Some(PointerEdit::Create);
                ui.close();
            }
        });

        for block in &track.blocks {
            let left = beat_x(
                timeline,
                block.start_tick as f64 / TICKS_PER_BEAT as f64,
                state.view_start,
            );
            let right = beat_x(
                timeline,
                block.start_tick.saturating_add(block.length_ticks) as f64 / TICKS_PER_BEAT as f64,
                state.view_start,
            );
            // Entirely off-window: both edges clamp to the same side.
            if right - left < 1.0 {
                continue;
            }
            let rect = egui::Rect::from_min_max(
                egui::pos2(left + space::XXS, top + space::XS),
                egui::pos2(right - space::XXS, bottom - space::XS),
            );
            let name = input
                .song
                .pattern(block.pattern_id)
                .map_or("PATTERN", |pattern| pattern.name.as_str());
            draw_block(ui, rect, name, block.length_ticks);

            if state.selected_block == Some(block.id) {
                for leading in [true, false] {
                    let x = if leading { rect.left() } else { rect.right() };
                    let grip = egui::Rect::from_center_size(
                        egui::pos2(x, rect.center().y),
                        egui::vec2(8.0, rect.height()),
                    );
                    let response = ui
                        .interact(
                            grip,
                            ui.id().with(("arrangement-edge", block.id.0, leading)),
                            egui::Sense::drag(),
                        )
                        .affords(Affords::Sweep);
                    if response.hovered() || response.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if response.dragged()
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        let fine = ui.input(|input| input.modifiers.alt);
                        let raw_tick = x_tick(timeline, pointer.x, state.view_start);
                        let tick = if fine {
                            raw_tick
                        } else {
                            (raw_tick / TICKS_PER_BEAT) * TICKS_PER_BEAT
                        };
                        let old_end = block.start_tick.saturating_add(block.length_ticks);
                        let (start_tick, length_ticks) = if leading {
                            let start = tick.min(old_end.saturating_sub(1));
                            (start, old_end - start)
                        } else {
                            (
                                block.start_tick,
                                tick.max(block.start_tick + 1) - block.start_tick,
                            )
                        };
                        pointer_edit = Some(PointerEdit::Resize {
                            id: block.id,
                            start_tick,
                            length_ticks,
                        });
                    }
                }
            }
        }
    }

    if let Some(edit) = pointer_edit {
        state.notice = Some(match edit {
            PointerEdit::Create => {
                edit::apply(edit::Command::CreateClip, input.song, state.selection())
            }
            PointerEdit::Move {
                id,
                track,
                start_tick,
                copy,
            } => {
                if let Ok(placement) = edit::place_block(input.song, id, track, start_tick, copy) {
                    select_placement(state, placement);
                    "CLIP PLACED"
                } else {
                    "REGION OCCUPIED"
                }
            }
            PointerEdit::Resize {
                id,
                start_tick,
                length_ticks,
            } => {
                if let Ok(placement) = edit::resize_block(input.song, id, start_tick, length_ticks)
                {
                    select_placement(state, placement);
                    "CLIP RESIZED"
                } else {
                    "RESIZE BLOCKED"
                }
            }
        });
    }

    match rename_outcome {
        RenameOutcome::Editing => {}
        RenameOutcome::Cancel => state.rename = None,
        RenameOutcome::Commit => {
            if let Some(rename) = state.rename.take() {
                let text = rename.text.trim();
                if text.is_empty() {
                    state.refusal = Some("RENAME: EMPTY NAME".to_owned());
                } else if let Some(track) = input.song.tracks.get_mut(rename.track) {
                    track.name = text.to_owned();
                }
            }
        }
    }

    draw_selection(ui, timeline, state, focused);
    state.follow_playhead(input.playhead_beats, input.playing);
    draw_playhead(
        ui,
        ruler,
        timeline,
        input.playhead_beats,
        input.playing,
        state.view_start,
    );
    let sentence_display = (!voice.sentence.is_empty()).then(|| voice.sentence.display());
    let overlay = sentence_display.as_deref().or(state.refusal.as_deref());
    draw_status(ui, area, state.selection(), state.notice, overlay);
    focus::show(ui.painter(), area, focused);
    outcome
}

#[derive(Clone, Copy, PartialEq)]
enum RenameOutcome {
    Editing,
    Commit,
    Cancel,
}

/// The rename field over a track header: Enter commits, anything that
/// ends the edit without Enter — Escape, a click elsewhere — cancels.
fn rename_editor(ui: &mut egui::Ui, header: egui::Rect, rename: &mut TrackRename) -> RenameOutcome {
    let field = egui::Rect::from_min_max(
        egui::pos2(header.left() + space::SM, header.top() + space::XS),
        egui::pos2(header.right() - space::XS, header.center().y),
    );
    ui.painter().rect_filled(field, 0.0, egui::Color32::BLACK);
    let id = ui.id().with("track-rename");
    let response = ui.put(
        field.shrink(2.0),
        egui::TextEdit::singleline(&mut rename.text)
            .id(id)
            .font(egui::FontId::new(font::LABEL, egui::FontFamily::Monospace))
            .text_color(OUTLINE)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO),
    );
    if !response.has_focus() && !response.lost_focus() {
        ui.ctx().memory_mut(|memory| memory.request_focus(id));
    }
    if response.lost_focus() {
        if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            RenameOutcome::Commit
        } else {
            RenameOutcome::Cancel
        }
    } else {
        RenameOutcome::Editing
    }
}

fn keyboard(
    ui: &mut egui::Ui,
    voice: &mut Voice<'_>,
    state: &mut ArrangementState,
    song: &mut Song,
    outcome: &mut Outcome,
) {
    let Some(utterance) = voice.sentence.consume(ui.ctx()) else {
        return;
    };
    state.refusal = None;
    speak(state, song, voice.registers, utterance, outcome);
}

fn speak(
    state: &mut ArrangementState,
    song: &mut Song,
    registers: &mut Registers,
    utterance: Utterance,
    outcome: &mut Outcome,
) {
    let count = utterance.count as isize;
    match (utterance.verb, utterance.motion) {
        (None, Some(motion)) => {
            // A held motion extends the selection from an anchored corner;
            // a bare one travels and drops it. On this surface the hold
            // qualifies the SELECTION — there is no trig noun here to own
            // it, so the sentence recolours to "take this along".
            let (track, beat) = motion_delta(motion);
            state.move_cursor(song, track * count, beat * count, utterance.held);
        }
        (Some(Verb::Act), _) => {
            if state.selected_pattern(song).is_some() {
                outcome.open_pattern = true;
            } else {
                // Act on empty ground CREATES — the sequencer's
                // empty-step toggle, lifted to clips. A bare cursor
                // makes one bar; an extended selection makes exactly
                // itself. Act again opens what was just made.
                if state.selection().beat_count() <= 1 {
                    let beat = state.cursor.beat;
                    state.select_region(state.cursor.track, beat, beat + BEATS_PER_BAR);
                }
                state.notice = Some(edit::apply(
                    edit::Command::CreateClip,
                    song,
                    state.selection(),
                ));
            }
        }
        (Some(Verb::Delete), _) => {
            state.notice = Some(edit::apply(
                edit::Command::DeleteClips,
                song,
                state.selection(),
            ));
            state.selected_block = None;
        }
        (Some(Verb::Duplicate), _) => duplicate(state, song),
        (Some(Verb::Nudge), Some(motion)) => nudge(state, song, motion, count),
        (Some(Verb::Resize), Some(motion @ (Motion::Left | Motion::Right))) => {
            resize(state, song, motion, count);
        }
        (Some(Verb::Resize), Some(_)) => {
            state.refusal = Some("RESIZE: LEFT OR RIGHT".to_owned());
        }
        (Some(Verb::Mute), _) => match song.tracks.get_mut(state.cursor.track) {
            Some(track) => {
                if utterance.count % 2 == 1 {
                    track.muted = !track.muted;
                }
            }
            None => state.refusal = Some("MUTE: NO TRACK".to_owned()),
        },
        (Some(Verb::Solo), _) => match song.tracks.get_mut(state.cursor.track) {
            Some(track) => {
                if utterance.count % 2 == 1 {
                    track.solo = !track.solo;
                }
            }
            None => state.refusal = Some("SOLO: NO TRACK".to_owned()),
        },
        (Some(Verb::Yank), _) => {
            let payload = state.selected_pattern(song).and_then(|pattern_id| {
                let tick = state.cursor.beat * TICKS_PER_BEAT;
                let length_ticks = song
                    .tracks
                    .get(state.cursor.track)?
                    .blocks
                    .iter()
                    .find(|block| {
                        block.pattern_id == pattern_id
                            && block.start_tick <= tick
                            && tick < block.start_tick.saturating_add(block.length_ticks)
                    })?
                    .length_ticks;
                Some(ClipPayload {
                    pattern: song.pattern(pattern_id)?.clone(),
                    length_ticks,
                })
            });
            match payload {
                Some(clip) => {
                    registers.yank(Payload::Clip(clip));
                    state.refusal = Some("YANKED A CLIP".to_owned());
                }
                None => state.refusal = Some("YANK: NO CLIP".to_owned()),
            }
        }
        (Some(Verb::Put), _) => match registers.clip() {
            Ok(clip) => {
                let placed = song.adopt_pattern(
                    clip.pattern.clone(),
                    state.cursor.track,
                    state.cursor.beat * TICKS_PER_BEAT,
                    clip.length_ticks,
                );
                match placed {
                    Some(id) => {
                        let start = state.cursor.beat;
                        let end = start + clip.length_ticks.div_ceil(TICKS_PER_BEAT);
                        state.select_block(id, state.cursor.track, start, end);
                        state.notice = Some("CLIP PLACED");
                    }
                    None => state.refusal = Some("PUT: BLOCKED".to_owned()),
                }
            }
            Err(refusal) => state.refusal = Some(refusal),
        },
        (Some(Verb::Search), _) => state.palette.open(),
        (Some(Verb::Rename), _) => match song.tracks.get(state.cursor.track) {
            Some(track) => {
                state.rename = Some(TrackRename {
                    track: state.cursor.track,
                    text: track.name.clone(),
                });
            }
            None => state.refusal = Some("RENAME: NO TRACK".to_owned()),
        },
        (Some(verb), _) => state.refusal = Some(format!("{}: NOT HERE", verb.name())),
        (None, None) => {}
    }
}

fn motion_delta(motion: Motion) -> (isize, isize) {
    match motion {
        Motion::Left => (0, -1),
        Motion::Right => (0, 1),
        Motion::Up => (-1, 0),
        Motion::Down => (1, 0),
    }
}

fn duplicate(state: &mut ArrangementState, song: &mut Song) {
    let Some(id) = state.active_block(song) else {
        state.refusal = Some("DUPLICATE: NO CLIP".to_owned());
        return;
    };
    let Some((track, block)) = song.pattern_block(id) else {
        state.refusal = Some("DUPLICATE: NO CLIP".to_owned());
        return;
    };
    let start_tick = block.start_tick.saturating_add(block.length_ticks);
    match edit::place_block(song, id, track, start_tick, true) {
        Ok(placement) => {
            select_placement(state, placement);
            state.notice = Some("CLIP DUPLICATED");
        }
        Err(_) => state.refusal = Some("DUPLICATE: BLOCKED".to_owned()),
    }
}

fn nudge(state: &mut ArrangementState, song: &mut Song, motion: Motion, count: isize) {
    let Some(id) = state.active_block(song) else {
        state.refusal = Some("NUDGE: NO CLIP".to_owned());
        return;
    };
    let Some((track, block)) = song.pattern_block(id) else {
        state.refusal = Some("NUDGE: NO CLIP".to_owned());
        return;
    };
    let (track_delta, beat_delta) = motion_delta(motion);
    let Some(target_track) = track.checked_add_signed(track_delta * count) else {
        state.refusal = Some("NUDGE: BLOCKED".to_owned());
        return;
    };
    let Some(start_tick) = block
        .start_tick
        .checked_add_signed(beat_delta * count * TICKS_PER_BEAT as isize)
    else {
        state.refusal = Some("NUDGE: BLOCKED".to_owned());
        return;
    };
    match edit::place_block(song, id, target_track, start_tick, false) {
        Ok(placement) => {
            select_placement(state, placement);
            state.notice = Some("CLIP NUDGED");
        }
        Err(_) => state.refusal = Some("NUDGE: BLOCKED".to_owned()),
    }
}

fn resize(state: &mut ArrangementState, song: &mut Song, motion: Motion, count: isize) {
    let Some(id) = state.active_block(song) else {
        state.refusal = Some("RESIZE: NO CLIP".to_owned());
        return;
    };
    let Some((_, block)) = song.pattern_block(id) else {
        state.refusal = Some("RESIZE: NO CLIP".to_owned());
        return;
    };
    let direction = if motion == Motion::Left { -1 } else { 1 };
    let delta = direction * count * TICKS_PER_BEAT as isize;
    let length_ticks = block
        .length_ticks
        .saturating_add_signed(delta)
        .max(TICKS_PER_BEAT);
    match edit::resize_block(song, id, block.start_tick, length_ticks) {
        Ok(placement) => {
            select_placement(state, placement);
            state.notice = Some("CLIP RESIZED");
        }
        Err(_) => state.refusal = Some("RESIZE: BLOCKED".to_owned()),
    }
}

fn select_placement(state: &mut ArrangementState, placement: edit::Placement) {
    state.select_block(
        placement.id,
        placement.track,
        placement.start_tick / TICKS_PER_BEAT,
        placement
            .start_tick
            .saturating_add(placement.length_ticks)
            .div_ceil(TICKS_PER_BEAT),
    );
}

fn draw_ruler(ui: &egui::Ui, rect: egui::Rect, timeline: egui::Rect, view_start: usize) {
    ui.painter().rect_filled(rect, 0.0, RULER);
    ui.painter().rect_filled(
        egui::Rect::from_min_max(rect.min, egui::pos2(timeline.left(), rect.bottom())),
        0.0,
        SURFACE_FRAME,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(space::SM, 0.0),
        egui::Align2::LEFT_CENTER,
        "SONG // ARRANGE  [GRID 1/4]",
        egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
        OUTLINE,
    );
    if timeline.left() - rect.left() >= 300.0 {
        ui.painter().text(
            egui::pos2(timeline.left() - space::SM, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "ARROWS LOCATE  /  ENTER OPEN",
            egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
            MUTED,
        );
    }
    let bar_width = timeline.width() / SONG_BARS as f32;
    let label_stride = if bar_width >= 30.0 {
        1
    } else if bar_width >= 18.0 {
        2
    } else {
        4
    };
    let first_bar = view_start / BEATS_PER_BAR;
    for bar in 0..=SONG_BARS {
        let x = beat_x(
            timeline,
            ((first_bar + bar) * BEATS_PER_BAR) as f64,
            view_start,
        );
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.bottom() - 8.0),
                egui::pos2(x, rect.bottom()),
            ],
            egui::Stroke::new(stroke::HAIR, MUTED),
        );
        if bar < SONG_BARS && bar % label_stride == 0 {
            ui.painter().text(
                egui::pos2(x + space::XS, rect.center().y),
                egui::Align2::LEFT_CENTER,
                format!("{:02}", first_bar + bar + 1),
                egui::FontId::new(font::MINI_LABEL, egui::FontFamily::Monospace),
                MUTED,
            );
        }
    }
}

fn draw_track(
    ui: &egui::Ui,
    header: egui::Rect,
    lane: egui::Rect,
    index: usize,
    name: &str,
    kind: &TrackKind,
    muted: bool,
    solo: bool,
    any_solo: bool,
) {
    ui.painter().rect_filled(header, 0.0, TRACK_HEADER);
    ui.painter().rect_filled(lane, 0.0, TRACK_LANE);
    ui.painter().text(
        header.left_center() + egui::vec2(space::SM, -space::XS),
        egui::Align2::LEFT_CENTER,
        format!("{:02}  {name}", index + 1),
        egui::FontId::new(font::LABEL, egui::FontFamily::Monospace),
        track_name_ink(muted, solo, any_solo),
    );
    let state_sign = match (muted, solo) {
        (true, true) => " · M S",
        (true, false) => " · M",
        (false, true) => " · S",
        (false, false) => "",
    };
    ui.painter().text(
        header.left_center() + egui::vec2(space::SM, space::MD),
        egui::Align2::LEFT_CENTER,
        format!(
            "{}{}",
            match kind {
                TrackKind::Instrument => "MIDI / INSTRUMENT",
                TrackKind::Audio => "AUDIO",
            },
            state_sign
        ),
        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
        MUTED,
    );
    for beat in 0..=SONG_BEATS {
        let x = lane.left() + lane.width() * beat as f32 / SONG_BEATS as f32;
        let color = if beat % BEATS_PER_BAR == 0 {
            QUIET
        } else {
            egui::Color32::from_gray(16)
        };
        ui.painter().line_segment(
            [egui::pos2(x, lane.top()), egui::pos2(x, lane.bottom())],
            egui::Stroke::new(stroke::HAIR, color),
        );
    }
}

fn track_name_ink(muted: bool, solo: bool, any_solo: bool) -> egui::Color32 {
    if solo {
        OUTLINE
    } else if muted || any_solo {
        SILENT
    } else {
        ACTIVE
    }
}

fn draw_block(ui: &egui::Ui, rect: egui::Rect, name: &str, length_ticks: usize) {
    ui.painter().rect_filled(rect, 0.0, BLOCK);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 3.0)),
        0.0,
        MUTED,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(space::SM, -space::XS),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::new(font::BODY, egui::FontFamily::Monospace),
        OUTLINE,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(space::SM, space::MD),
        egui::Align2::LEFT_CENTER,
        format!("64 STEP  /  {} BEAT", length_ticks / TICKS_PER_BEAT),
        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
        MUTED,
    );
}

fn draw_selection(ui: &egui::Ui, timeline: egui::Rect, state: &ArrangementState, focused: bool) {
    let view_start = state.view_start;
    let selection = state.selection();
    let rect = egui::Rect::from_min_max(
        egui::pos2(
            beat_x(timeline, selection.first_beat as f64, view_start),
            timeline.top() + selection.first_track as f32 * TRACK_H,
        ),
        egui::pos2(
            beat_x(timeline, selection.end_beat as f64, view_start),
            (timeline.top() + (selection.last_track + 1) as f32 * TRACK_H).min(timeline.bottom()),
        ),
    )
    .shrink(2.0);
    draw_block_cursor(ui.painter(), rect, focused);

    let active_left = beat_x(timeline, state.cursor.beat as f64, view_start);
    let active_right = beat_x(timeline, (state.cursor.beat + 1) as f64, view_start);
    let active_x = if state.cursor.beat == selection.first_beat {
        active_left
    } else {
        active_right
    };
    ui.painter().line_segment(
        [
            egui::pos2(active_x, rect.top()),
            egui::pos2(active_x, rect.bottom()),
        ],
        egui::Stroke::new(stroke::FOCUS, OUTLINE),
    );
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(active_x - 4.0, rect.top()),
            egui::pos2(active_x + 4.0, rect.top()),
            egui::pos2(active_x, rect.top() + 6.0),
        ],
        OUTLINE,
        egui::Stroke::NONE,
    ));
}

fn draw_block_cursor(painter: &egui::Painter, rect: egui::Rect, focused: bool) {
    let cap = 12.0;
    let ink = egui::Stroke::new(
        if focused { stroke::FOCUS } else { stroke::HAIR },
        if focused { OUTLINE } else { MUTED },
    );
    for (from, to) in [
        (rect.left_top(), rect.left_top() + egui::vec2(cap, 0.0)),
        (rect.left_top(), rect.left_top() + egui::vec2(0.0, cap)),
        (rect.right_top() - egui::vec2(cap, 0.0), rect.right_top()),
        (rect.right_top(), rect.right_top() + egui::vec2(0.0, cap)),
        (
            rect.left_bottom(),
            rect.left_bottom() + egui::vec2(cap, 0.0),
        ),
        (
            rect.left_bottom() - egui::vec2(0.0, cap),
            rect.left_bottom(),
        ),
        (
            rect.right_bottom() - egui::vec2(cap, 0.0),
            rect.right_bottom(),
        ),
        (
            rect.right_bottom() - egui::vec2(0.0, cap),
            rect.right_bottom(),
        ),
    ] {
        painter.line_segment([from, to], ink);
    }
}

fn draw_status(
    ui: &egui::Ui,
    area: egui::Rect,
    selection: super::state::Selection,
    notice: Option<&str>,
    overlay: Option<&str>,
) {
    let message = notice.map_or_else(
        || {
            format!(
                "T{:02}  {:02}.{}—{:02}.{}  /  {} BEAT",
                selection.first_track + 1,
                selection.first_beat / BEATS_PER_BAR + 1,
                selection.first_beat % BEATS_PER_BAR + 1,
                (selection.end_beat - 1) / BEATS_PER_BAR + 1,
                (selection.end_beat - 1) % BEATS_PER_BAR + 1,
                selection.beat_count(),
            )
        },
        str::to_owned,
    );
    ui.painter().text(
        area.left_bottom() + egui::vec2(space::SM, -space::SM),
        egui::Align2::LEFT_BOTTOM,
        message,
        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
        MUTED,
    );
    ui.painter().text(
        area.right_bottom() - egui::vec2(space::SM, space::SM),
        egui::Align2::RIGHT_BOTTOM,
        overlay.unwrap_or("ARROWS LOCATE  /  ENTER ACT  /  / SEARCH"),
        egui::FontId::new(font::MICRO_LABEL, egui::FontFamily::Monospace),
        MUTED,
    );
}

fn draw_playhead(
    ui: &egui::Ui,
    ruler: egui::Rect,
    timeline: egui::Rect,
    playhead_beats: f64,
    playing: bool,
    view_start: usize,
) {
    let window = view_start as f64..(view_start + SONG_BEATS) as f64;
    if !window.contains(&playhead_beats) {
        return;
    }
    let x = beat_x(timeline, playhead_beats, view_start);
    let color = if playing { OUTLINE } else { MUTED };
    ui.painter().line_segment(
        [
            egui::pos2(x, ruler.bottom()),
            egui::pos2(x, timeline.bottom()),
        ],
        egui::Stroke::new(stroke::HAIR, color),
    );
    ui.painter().add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(x - 4.0, ruler.bottom() - 7.0),
            egui::pos2(x + 4.0, ruler.bottom() - 7.0),
            egui::pos2(x, ruler.bottom()),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

fn beat_x(rect: egui::Rect, beat: f64, view_start: usize) -> f32 {
    let relative = beat - view_start as f64;
    rect.left() + rect.width() * (relative / SONG_BEATS as f64).clamp(0.0, 1.0) as f32
}

fn x_beat(rect: egui::Rect, x: f32, view_start: usize) -> usize {
    view_start
        + (((x - rect.left()) / rect.width()).clamp(0.0, 0.999_999) * SONG_BEATS as f32) as usize
}

fn x_raw_beat(rect: egui::Rect, x: f32, view_start: usize) -> f32 {
    view_start as f32 + ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) * SONG_BEATS as f32
}

fn x_tick(rect: egui::Rect, x: f32, view_start: usize) -> usize {
    (x_raw_beat(rect, x, view_start) * TICKS_PER_BEAT as f32).round() as usize
}

fn y_track(timeline: egui::Rect, y: f32, track_count: usize) -> usize {
    (((y - timeline.top()) / TRACK_H).floor().max(0.0) as usize).min(track_count.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utter(
        state: &mut ArrangementState,
        song: &mut Song,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> Outcome {
        let mut registers = Registers::default();
        utter_with(state, song, &mut registers, verb, motion, count)
    }

    fn utter_with(
        state: &mut ArrangementState,
        song: &mut Song,
        registers: &mut Registers,
        verb: Option<Verb>,
        motion: Option<Motion>,
        count: usize,
    ) -> Outcome {
        let mut outcome = Outcome::default();
        state.refusal = None;
        speak(
            state,
            song,
            registers,
            Utterance {
                count,
                verb,
                motion,
                held: false,
            },
            &mut outcome,
        );
        outcome
    }

    /// A clip yanked here is a deep copy: put lands it elsewhere as a NEW
    /// pattern, and it survives the deletion of its source. The trip also
    /// proves the typed register end-to-end across panels.
    #[test]
    fn yank_survives_source_deletion_and_puts_as_a_new_pattern() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();
        let mut registers = Registers::default();

        utter_with(
            &mut state,
            &mut song,
            &mut registers,
            Some(Verb::Yank),
            None,
            1,
        );
        assert_eq!(state.refusal.as_deref(), Some("YANKED A CLIP"));

        // Delete garbage-collects the now-unreferenced source pattern —
        // only the register's deep copy remains.
        utter_with(
            &mut state,
            &mut song,
            &mut registers,
            Some(Verb::Delete),
            None,
            1,
        );
        assert!(song.tracks[0].blocks.is_empty(), "the source is gone");
        assert!(song.patterns.is_empty(), "its pattern went with it");

        state.cursor.beat = 0;
        utter_with(
            &mut state,
            &mut song,
            &mut registers,
            Some(Verb::Put),
            None,
            1,
        );
        assert_eq!(song.tracks[0].blocks.len(), 1, "the copy lands anyway");
        assert_eq!(song.patterns.len(), 1);

        // A second put is a second independent pattern: puts never alias.
        state.cursor.beat = 20;
        utter_with(
            &mut state,
            &mut song,
            &mut registers,
            Some(Verb::Put),
            None,
            1,
        );
        assert_eq!(song.patterns.len(), 2);
        assert_ne!(song.patterns[0].id, song.patterns[1].id);

        // And yanking where no clip lives refuses by name.
        state.cursor.beat = 40;
        let mut empty = Registers::default();
        utter_with(&mut state, &mut song, &mut empty, Some(Verb::Yank), None, 1);
        assert_eq!(state.refusal.as_deref(), Some("YANK: NO CLIP"));
    }

    /// Rename enters TYPING on the track under the cursor, pre-filled
    /// with the current name so editing starts from the truth.
    #[test]
    fn rename_opens_typing_on_the_cursor_track() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();
        utter(&mut state, &mut song, Some(Verb::Rename), None, 1);
        let rename = state.rename.as_ref().expect("typing begins");
        assert_eq!(rename.track, 0);
        assert_eq!(rename.text, song.tracks[0].name);
        assert_eq!(state.refusal, None);
    }

    /// Act on empty ground creates: a bare cursor mints one bar, an
    /// extended selection mints exactly itself, and act again opens it.
    #[test]
    fn act_on_empty_ground_creates_a_clip_then_opens_it() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();
        state.cursor.beat = 20; // past the default block (16 beats)

        let outcome = utter(&mut state, &mut song, Some(Verb::Act), None, 1);
        assert!(!outcome.open_pattern);
        assert_eq!(state.notice, Some("CLIP CREATED"));
        let block = song.tracks[0]
            .blocks
            .iter()
            .find(|block| block.start_tick == 20 * TICKS_PER_BEAT)
            .expect("a bar-long block appears at the cursor");
        assert_eq!(block.length_ticks, 4 * TICKS_PER_BEAT);

        let outcome = utter(&mut state, &mut song, Some(Verb::Act), None, 1);
        assert!(outcome.open_pattern, "act again opens what was made");
    }

    /// Held motion drags a selection corner along; the next bare motion
    /// drops it. The anchor never moves — only the cursor travels.
    #[test]
    fn held_motion_extends_the_selection_and_bare_motion_drops_it() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();
        let mut registers = Registers::default();
        let mut outcome = Outcome::default();
        speak(
            &mut state,
            &mut song,
            &mut registers,
            Utterance {
                count: 4,
                verb: None,
                motion: Some(Motion::Right),
                held: true,
            },
            &mut outcome,
        );
        let selection = state.selection();
        assert_eq!(selection.first_beat, 0);
        assert_eq!(selection.end_beat, 5, "anchor stays, cursor travels");

        utter(&mut state, &mut song, None, Some(Motion::Right), 1);
        assert_eq!(state.selection().beat_count(), 1, "bare motion drops it");
    }

    #[test]
    fn counted_bare_motion_moves_only_the_clip_cursor() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();
        let original = song.tracks[0].blocks[0].clone();

        utter(&mut state, &mut song, None, Some(Motion::Right), 4);

        assert_eq!(state.cursor.beat, 4);
        assert_eq!(song.tracks[0].blocks[0], original);
    }

    #[test]
    fn act_opens_the_clip_under_the_cursor() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();

        let outcome = utter(&mut state, &mut song, Some(Verb::Act), None, 1);

        assert!(outcome.open_pattern);
        assert!(state.refusal.is_none());
    }

    #[test]
    fn delete_uses_the_existing_selection_edit_path() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();

        utter(&mut state, &mut song, Some(Verb::Delete), None, 1);

        assert!(song.tracks[0].blocks.is_empty());
        assert_eq!(state.notice, Some("CLIP DELETED"));
    }

    #[test]
    fn counted_nudge_and_resize_use_grid_units() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();
        let id = song.tracks[0].blocks[0].id;
        let original_length = song.tracks[0].blocks[0].length_ticks;

        utter(
            &mut state,
            &mut song,
            Some(Verb::Nudge),
            Some(Motion::Right),
            2,
        );
        let (_, moved) = song.pattern_block(id).expect("nudge keeps the clip");
        assert_eq!(moved.start_tick, 2 * TICKS_PER_BEAT);

        utter(
            &mut state,
            &mut song,
            Some(Verb::Resize),
            Some(Motion::Left),
            3,
        );
        let (_, resized) = song.pattern_block(id).expect("resize keeps the clip");
        assert_eq!(resized.length_ticks, original_length - 3 * TICKS_PER_BEAT);
    }

    #[test]
    fn unsupported_verbs_and_invalid_resize_motion_refuse_out_loud() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();

        utter(&mut state, &mut song, Some(Verb::Condition), None, 1);
        assert_eq!(state.refusal.as_deref(), Some("CONDITION: NOT HERE"));

        utter(
            &mut state,
            &mut song,
            Some(Verb::Resize),
            Some(Motion::Up),
            1,
        );
        assert_eq!(state.refusal.as_deref(), Some("RESIZE: LEFT OR RIGHT"));
    }

    #[test]
    fn mute_and_solo_act_on_the_track_under_the_cursor() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();

        utter(&mut state, &mut song, Some(Verb::Mute), None, 1);
        assert!(song.tracks[0].muted);
        assert!(state.refusal.is_none(), "a valid mute must not refuse");

        utter(&mut state, &mut song, Some(Verb::Solo), None, 1);
        assert!(song.tracks[0].solo);
        assert!(state.refusal.is_none(), "a valid solo must not refuse");

        utter(&mut state, &mut song, Some(Verb::Mute), None, 2);
        assert!(song.tracks[0].muted, "two toggles return to the same state");
    }

    #[test]
    fn silent_and_soloed_tracks_use_the_value_ladder() {
        assert_eq!(track_name_ink(false, false, false), ACTIVE);
        assert_eq!(track_name_ink(true, false, false), SILENT);
        assert_eq!(track_name_ink(false, false, true), SILENT);
        assert_eq!(track_name_ink(false, true, true), OUTLINE);
        assert!(SILENT.r() < ACTIVE.r());
        assert!(ACTIVE.r() < OUTLINE.r());
    }

    #[test]
    fn search_enters_the_existing_arrangement_palette() {
        let mut song = Song::default();
        let mut state = ArrangementState::default();

        utter(&mut state, &mut song, Some(Verb::Search), None, 1);

        assert!(state.palette.open);
    }
}
