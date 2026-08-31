//! Editing an audio clip: the commands that reach into a file, and the
//! renders they queue.
//!
//! Every one of these is destructive in the sense that matters — it makes
//! a NEW file and repoints the clip at it — so they share one road out
//! through `request_render` and one road back through `pump_renders`.
//! Nothing here edits samples in place, and nothing blocks: a render runs
//! on a worker and the clip changes when it lands.

use super::*;

impl App {
    /// What a destructive verb would act on: a range of SOURCE frames in
    /// the clip's forward file, and the channels it covers.
    ///
    /// `None` when there is nothing selected, or when the selection
    /// crosses a looped clip's seam — where the material either side is
    /// two different stretches of the file and there is no honest single
    /// range. Every verb asks this, and every verb is disabled with that
    /// reason when it comes back empty.
    pub(crate) fn audio_edit_target(&self) -> Option<(u64, u64, daw::render::Channels)> {
        let selection = self.waveform.selection()?;
        let audio = self.arrangement.active_audio_clip()?.audio.as_ref()?;
        let end = waveform::region_end(audio, audio.file_frames());
        let (from, to) = waveform::source_span(audio, end, selection.from, selection.to)?;
        Some((from, to, selection.channels.into()))
    }

    /// The destructive verbs, all of which are one render of one job.
    ///
    /// Gathered rather than spread through the palette's match for the
    /// same reason the note commands are: they share a target, a refusal
    /// and a shape, and a reader looking for "what can this do to audio?"
    /// should find the list in one place.
    pub(crate) fn run_audio_command(&mut self, id: &str) {
        use daw::render::{FadeDir, Normalize, Op};
        // View verbs first: they need no target and must work with
        // nothing selected.
        match id {
            "audio.select_all" => return self.waveform.select_all(),
            "audio.select_none" => return self.waveform.select_none(),
            "audio.snap_zero" => return self.waveform.toggle_snap_zero(),
            "audio.db_scale" => return self.waveform.toggle_decibels(),
            "audio.zoom_selection" => return self.waveform.zoom_to_selection(),
            "audio.ripple" => return self.waveform.toggle_ripple(),
            "audio.envelope" => return self.waveform.toggle_envelope(),
            "audio.envelope_clear" => {
                return self.apply_clip_edit(waveform::ClipEdit::Envelope(Vec::new()));
            }
            _ => {}
        }
        if matches!(
            id,
            "audio.swap_channels"
                | "audio.mono"
                | "audio.mono_left"
                | "audio.mono_right"
                | "audio.extract_left"
                | "audio.extract_right"
        ) {
            return self.run_audio_channel_command(id);
        }
        if id == "audio.flatten" {
            return self.flatten_audio_clip();
        }
        if let Some(semitones) = match id {
            "audio.transpose_up" => Some(1.0),
            "audio.transpose_down" => Some(-1.0),
            "audio.transpose_octave_up" => Some(12.0),
            "audio.transpose_octave_down" => Some(-12.0),
            _ => None,
        } {
            return self.transpose_audio_clip(semitones);
        }
        // The structural verbs change the file's LENGTH, so they carry
        // consequences for the clip's placement that the level verbs do
        // not. They are handled apart for that reason, not for tidiness.
        if matches!(
            id,
            "audio.copy"
                | "audio.cut"
                | "audio.paste"
                | "audio.crop"
                | "audio.insert_silence"
                | "audio.delete"
        ) {
            return self.run_audio_structural_command(id);
        }
        let Some((from, to, channels)) = self.audio_edit_target() else {
            self.notice =
                Some("nothing to edit: select a range inside one pass of the clip".into());
            return;
        };
        let curve = daw::params::clip::Curve::LINEAR;
        let (verb, op) = match id {
            "audio.silence" => ("silence", Op::Silence { from, to, channels }),
            "audio.gain" => (
                "gain",
                Op::Gain {
                    from,
                    to,
                    channels,
                    db: self.waveform.edit_gain_db(),
                },
            ),
            // −0.1 dBFS rather than 0: a file normalized to exactly full
            // scale has inter-sample peaks above it, and every converter
            // and lossy encoder downstream will find them.
            "audio.normalize" => (
                "normalize",
                Op::Norm {
                    from,
                    to,
                    channels,
                    target_db: -0.1,
                    mode: Normalize::Peak,
                    allow_clipping: false,
                },
            ),
            "audio.normalize_rms" => (
                "normalize",
                Op::Norm {
                    from,
                    to,
                    channels,
                    target_db: -18.0,
                    mode: Normalize::Rms,
                    allow_clipping: false,
                },
            ),
            "audio.reverse_sel" => ("reverse", Op::Reverse { from, to, channels }),
            "audio.invert" => ("invert", Op::Invert { from, to, channels }),
            "audio.remove_dc" => ("DC removal", Op::RemoveDc { from, to, channels }),
            "audio.fade_in" => (
                "fade in",
                Op::Fade {
                    from,
                    to,
                    channels,
                    dir: FadeDir::In,
                    curve,
                },
            ),
            "audio.fade_out" => (
                "fade out",
                Op::Fade {
                    from,
                    to,
                    channels,
                    dir: FadeDir::Out,
                    curve,
                },
            ),
            _ => return,
        };
        if !self.request_render(verb, vec![op], None) {
            self.notice = Some(format!("{verb} refused: a render is already running"));
        }
    }

    /// Print the clip's non-destructive state into a plain file.
    ///
    /// Everything the node does at playback — the region, the reversal,
    /// the loop's repeats, the envelope, both fades with their shapes,
    /// and the gain — becomes samples on disk, and every parameter goes
    /// back to neutral. Afterwards the file IS what you hear, which is
    /// the point: a flattened clip can be handed to anything.
    ///
    /// The ORDER below is the node's order. Get it wrong and the result
    /// is plausible and different: fading before the loop repeats would
    /// fade the first pass instead of the clip.
    pub(crate) fn flatten_audio_clip(&mut self) {
        use daw::params::clip::Curve;
        use daw::render::{FadeDir, Op};
        let Some(clip) = self.arrangement.active_audio_clip() else {
            return;
        };
        let Some(audio) = clip.audio.as_ref() else {
            return;
        };
        let span = waveform::clip_span_frames(clip, audio, self.transport.bpm);
        if span == 0 {
            self.notice = Some("a clip with no length has nothing to flatten".into());
            return;
        }
        let region_end = audio.source_offset.saturating_add(audio.source_frames);
        let region = audio.source_frames;
        let mut ops = vec![Op::Crop {
            from: audio.source_offset,
            to: region_end,
        }];
        if audio.reversed {
            ops.push(Op::Reverse {
                from: 0,
                to: region,
                channels: daw::render::Channels::all(),
            });
        }
        ops.push(Op::Fit {
            frames: span,
            looped: audio.looped,
        });
        if !audio.envelope.is_empty() {
            ops.push(Op::Envelope(std::sync::Arc::new(
                audio
                    .envelope
                    .iter()
                    .map(|(at, db)| (*at, envelope_gain(*db)))
                    .collect(),
            )));
        }
        // The fades AFTER the fit, because a fade is measured against the
        // clip's timeline span and not against one pass of its material.
        if audio.fade_in > 0 {
            ops.push(Op::Fade {
                from: 0,
                to: audio.fade_in.min(span),
                channels: daw::render::Channels::all(),
                dir: FadeDir::In,
                curve: Curve::new(audio.fade_in_curve),
            });
        }
        if audio.fade_out > 0 {
            ops.push(Op::Fade {
                from: span.saturating_sub(audio.fade_out),
                to: span,
                channels: daw::render::Channels::all(),
                dir: FadeDir::Out,
                curve: Curve::new(audio.fade_out_curve),
            });
        }
        if (audio.gain - 1.0).abs() > 1e-6 {
            ops.push(Op::Gain {
                from: 0,
                to: span,
                channels: daw::render::Channels::all(),
                db: if audio.gain > 0.0 {
                    20.0 * audio.gain.log10()
                } else {
                    -200.0
                },
            });
        }
        if !self.request_render("flatten", ops, None) {
            self.notice = Some("flatten refused: a render is already running".into());
            return;
        }
        if let Some(request) = self.render_job.as_mut() {
            request.flatten = true;
        }
    }

    /// Transpose by resampling — a tape machine, so the clip gets shorter
    /// as it gets higher.
    ///
    /// It CANNOT be previewed: there is no live resampler in the node, so
    /// this is a render like any other destructive verb, and the button
    /// says what it will do rather than pretending to be a knob.
    pub(crate) fn transpose_audio_clip(&mut self, semitones: f32) {
        let Some(audio) = self
            .arrangement
            .active_audio_clip()
            .and_then(|clip| clip.audio.as_ref())
        else {
            return;
        };
        let before = audio.file_frames();
        let ratio = daw::render::Op::transpose_ratio(semitones);
        let after = (before as f32 / ratio).round().max(1.0) as u64;
        // The whole file changes length, so the region change lands at
        // frame zero — and the clip's own span follows it under ripple.
        let change = Some((0u64, after as i64 - before as i64));
        if !self.request_render(
            "transpose",
            vec![daw::render::Op::Transpose { semitones }],
            change,
        ) {
            self.notice = Some("transpose refused: a render is already running".into());
            return;
        }
        if let Some(request) = self.render_job.as_mut() {
            request.rescale = Some(ratio);
        }
    }

    /// Swap, fold and extract.
    ///
    /// Whole-file operations, so they take no selection. Extract is the
    /// odd one: it leaves the original alone and puts its output in a NEW
    /// clip on the same lane, which is what "extract" means everywhere
    /// else and what makes it non-destructive despite being a render.
    pub(crate) fn run_audio_channel_command(&mut self, id: &str) {
        use daw::render::{Mono, Op};
        let (verb, op, as_new) = match id {
            "audio.swap_channels" => ("swap channels", Op::SwapChannels, false),
            "audio.mono" => ("fold to mono", Op::ToMono(Mono::Average), false),
            "audio.mono_left" => ("fold to mono", Op::ToMono(Mono::Left), false),
            "audio.mono_right" => ("fold to mono", Op::ToMono(Mono::Right), false),
            "audio.extract_left" => ("extract left", Op::TakeChannel(0), true),
            "audio.extract_right" => ("extract right", Op::TakeChannel(1), true),
            _ => return,
        };
        if !self.request_render(verb, vec![op], None) {
            self.notice = Some(format!("{verb} refused: a render is already running"));
            return;
        }
        if let Some(request) = self.render_job.as_mut() {
            request.as_new_clip = as_new;
        }
    }

    /// Cut, copy, paste, delete, crop and insert-silence.
    ///
    /// Copy reads the file on the worker rather than the UI thread, and a
    /// CUT sends the read and the render together: they both read the
    /// file as it is now, the worker runs them in order, and the
    /// clipboard is therefore filled from the material the cut is about
    /// to remove.
    pub(crate) fn run_audio_structural_command(&mut self, id: &str) {
        use daw::render::Op;
        let Some((from, to, channels)) = self.audio_edit_target() else {
            self.notice =
                Some("nothing to edit: select a range inside one pass of the clip".into());
            return;
        };
        let Some(audio) = self
            .arrangement
            .active_audio_clip()
            .and_then(|clip| clip.audio.as_ref())
        else {
            return;
        };
        let path = audio.path.clone();
        let source_channels = usize::from(self.audio_channels());
        let cursor_source = self.cursor_source_frame();

        match id {
            "audio.copy" => {
                self.wav_import_service.extract(path, from, to);
            }
            "audio.delete" => {
                // Silence in place: the length holds and nothing else on
                // the lane moves. Ctrl+X is the one that removes.
                self.request_render("delete", vec![Op::Silence { from, to, channels }], None);
            }
            "audio.cut" => {
                // A CHANNEL-MASKED CUT IS REFUSED. Making one side of a
                // stereo file shorter than the other is not a file.
                if channels != daw::render::Channels::all()
                    && source_channels > 1
                    && channels.0.count_ones() < source_channels as u32
                {
                    self.notice =
                        Some("cut needs every channel: silence is what one channel can do".into());
                    return;
                }
                self.wav_import_service.extract(path, from, to);
                let op = Op::Cut { from, to };
                let change = op.length_change(source_channels);
                self.request_render("cut", vec![op], change);
            }
            "audio.crop" => {
                let op = Op::Crop { from, to };
                let change = op.length_change(source_channels);
                self.request_render("crop", vec![op], change);
            }
            "audio.insert_silence" => {
                let Some(at) = cursor_source else {
                    self.notice = Some("the cursor is not over this clip's material".into());
                    return;
                };
                let op = Op::InsertSilence {
                    at,
                    frames: to.saturating_sub(from),
                };
                let change = op.length_change(source_channels);
                self.request_render("insert silence", vec![op], change);
            }
            "audio.paste" => {
                let Some(clipboard) = self.audio_clipboard.clone() else {
                    return;
                };
                if usize::from(clipboard.channels) != source_channels {
                    self.notice = Some(format!(
                        "paste refused: {} channels into {source_channels}",
                        clipboard.channels
                    ));
                    return;
                }
                let Some(at) = cursor_source else {
                    self.notice = Some("the cursor is not over this clip's material".into());
                    return;
                };
                let op = Op::Insert {
                    at,
                    material: clipboard.samples,
                    material_rate: clipboard.sample_rate,
                };
                let change = op.length_change(source_channels);
                self.request_render("paste", vec![op], change);
            }
            _ => {}
        }
    }

    /// How many channels the selected clip's file has, off the peak
    /// analysis — the only thing that has actually read the file.
    pub(crate) fn audio_channels(&self) -> u16 {
        self.arrangement
            .active_audio_clip()
            .and_then(|clip| clip.audio.as_ref())
            .and_then(|audio| self.waveform_cache.get(&audio.path))
            .map_or(1, |peaks| peaks.channels() as u16)
    }

    /// The edit cursor as a SOURCE frame, or `None` when it is not over
    /// this clip's material at all.
    pub(crate) fn cursor_source_frame(&self) -> Option<u64> {
        let audio = self.arrangement.active_audio_clip()?.audio.as_ref()?;
        let end = waveform::region_end(audio, audio.file_frames());
        let cursor = self
            .waveform
            .selection()
            .map_or(0, |selection| selection.from);
        waveform::source_frame_of(audio, end, cursor)
    }

    /// Ask for a destructive edit on the selected audio clip.
    ///
    /// Returns whether the ask was taken. It is refused when there is no
    /// audio clip, when a render is already in flight, or when the ops
    /// list is empty — a verb that would do nothing should say so rather
    /// than spend a second proving it.
    pub(crate) fn request_render(
        &mut self,
        verb: &'static str,
        ops: Vec<daw::render::Op>,
        length_change: Option<(u64, i64)>,
    ) -> bool {
        let Some(clip) = self.arrangement.active_audio_clip() else {
            return false;
        };
        let Some(audio) = clip.audio.as_ref() else {
            return false;
        };
        // The FORWARD file, always. A reversed clip plays a cached
        // reversal, but the edit belongs to the material the project
        // actually owns — and `waveform::source_span` already hands back
        // forward-file frames for exactly this reason.
        let source = audio.path.clone();
        let id = clip.id;
        self.request_render_from(verb, id, &source, ops, length_change)
    }

    /// The same, rendering FROM a named file — the original a clip pitch
    /// change renders from, never the previous pitch render: rendering a
    /// render would compound the resampling loss every time the knob
    /// moved.
    fn request_render_from(
        &mut self,
        verb: &'static str,
        clip: u64,
        source: &std::path::Path,
        ops: Vec<daw::render::Op>,
        length_change: Option<(u64, i64)>,
    ) -> bool {
        if ops.is_empty() || self.render_job.is_some() {
            return false;
        }
        let job = daw::render::Job {
            source: source.to_path_buf(),
            ops,
        };
        self.render_job = Some(RenderRequest {
            clip,
            length_change,
            verb,
            flatten: false,
            rescale: None,
            transpose_apply: None,
            as_new_clip: false,
            ripple: self.waveform.ripple(),
        });
        self.wav_import_service.render(job);
        true
    }

    /// Set the active clip's pitch — Live's Transpose + Detune, combined
    /// into one varispeed ratio. The knobs update immediately; the SOUND
    /// follows when the render lands, because no live resampler exists —
    /// the same honesty the destructive transpose verb keeps. Returning
    /// to zero is free and instant: the original file was never touched.
    pub(crate) fn set_clip_transpose(&mut self, transpose: f32, detune: f32) {
        let transpose = transpose.clamp(-48.0, 48.0);
        let detune = detune.clamp(-50.0, 50.0);
        let Some(clip) = self.arrangement.active_audio_clip() else {
            return;
        };
        let Some(audio) = clip.audio.as_ref() else {
            return;
        };
        let old_semitones = audio.transpose;
        let old_cents = audio.detune;
        let old_ratio = audio.applied_ratio.max(1e-4);
        let total = (transpose + detune / 100.0).clamp(-48.0, 48.0);
        let current = old_semitones + old_cents / 100.0;
        if (total - current).abs() < 1e-4 {
            return;
        }

        // Back to neutral: no render — hand the clip its original file
        // and its original frames. The fades and envelope were scaled
        // into render space, so they scale back by the old ratio.
        if total.abs() < 1e-4 {
            let Some(base) = audio.transposed_from.clone() else {
                return;
            };
            let Some((track, index)) = self.arrangement.selected_clip else {
                return;
            };
            let Some(clip) = self
                .arrangement
                .clips
                .get_mut(track)
                .and_then(|clips| clips.get_mut(index))
            else {
                return;
            };
            {
                let Some(audio) = clip.audio.as_mut() else {
                    return;
                };
                let frames = (audio.file_frames() as f32 * old_ratio).round().max(1.0) as u64;
                audio.path = base.clone();
                audio.source_offset = 0;
                audio.source_frames = frames;
                audio.file_frames = frames;
                audio.transpose = 0.0;
                audio.detune = 0.0;
                audio.transposed_from = None;
                audio.applied_ratio = 1.0;
            }
            rescale_after_transpose(clip, 1.0 / old_ratio);
            self.arrangement.force_recompile = true;
            let path = base;
            if self.waveform_cache.contains_key(&path) || self.waveform_pending.contains(&path) {
                return;
            }
            self.waveform_pending.insert(path.clone());
            self.waveform_service.request(path);
            return;
        }

        // A render, always from the ORIGINAL — the clip's own file while
        // it is un-pitched, and `transposed_from` once it is not.
        let base = audio
            .transposed_from
            .clone()
            .unwrap_or_else(|| audio.path.clone());
        let ratio = daw::render::Op::transpose_ratio(total);
        let before = audio.file_frames();
        let base_frames = (before as f32 * old_ratio).round().max(1.0) as u64;
        let after = (base_frames as f32 / ratio).round().max(1.0) as u64;
        let change = Some((0u64, after as i64 - before as i64));
        let clip_id = clip.id;
        if !self.request_render_from(
            "transpose",
            clip_id,
            &base,
            vec![daw::render::Op::Transpose { semitones: total }],
            change,
        ) {
            // The worker is busy: the knobs snap back to what is actually
            // sounding rather than promising a pitch that never arrives.
            let Some((track, index)) = self.arrangement.selected_clip else {
                return;
            };
            if let Some(audio) = self
                .arrangement
                .clips
                .get_mut(track)
                .and_then(|clips| clips.get_mut(index))
                .and_then(|clip| clip.audio.as_mut())
            {
                audio.transpose = old_semitones;
                audio.detune = old_cents;
            }
            self.notice = Some("transpose refused: a render is already running".into());
            return;
        }
        if let Some(request) = self.render_job.as_mut() {
            request.rescale = Some(ratio / old_ratio);
            request.transpose_apply = Some((ratio, base));
        }
        // The knobs say what was asked while the render runs; the model
        // fields follow so a second change reads the right old state.
        if let Some((track, index)) = self.arrangement.selected_clip
            && let Some(audio) = self
                .arrangement
                .clips
                .get_mut(track)
                .and_then(|clips| clips.get_mut(index))
                .and_then(|clip| clip.audio.as_mut())
        {
            audio.transpose = transpose;
            audio.detune = detune;
        }
    }

    /// Take the worker's answers: a rendered file to repoint a clip at,
    /// or material for the clipboard.
    pub(crate) fn pump_renders(&mut self) {
        while let Some(result) = self.wav_import_service.try_offline() {
            let rendered = match result {
                Ok(daw::library::Offline::Extracted(extract)) => {
                    self.notice = Some(format!("copied {} frames", extract.frames()));
                    self.audio_clipboard = Some(extract);
                    continue;
                }
                Ok(daw::library::Offline::Rendered(rendered)) => rendered,
                Err(error) => {
                    // The clip is untouched and still playing the file it
                    // was: a failed edit costs nothing but the message.
                    let verb = self
                        .render_job
                        .as_ref()
                        .map_or("audio edit", |request| request.verb);
                    self.notice = Some(format!("{verb} failed: {error}"));
                    self.render_job = None;
                    continue;
                }
            };
            let Some(mut request) = self.render_job.take() else {
                continue;
            };
            // On a LANE first, where a ripple has neighbours to move.
            let placed = self
                .arrangement
                .clips
                .iter()
                .enumerate()
                .find_map(|(track, clips)| {
                    clips
                        .iter()
                        .position(|clip| clip.id == request.clip)
                        .map(|index| (track, index))
                });
            match placed {
                Some((track, index)) if request.as_new_clip => {
                    // EXTRACT: the original is left exactly as it was and
                    // the render lands in a clip of its own, placed by
                    // the same gap-finding rule every other new clip uses.
                    let id = self.arrangement.next_id();
                    let clips = &mut self.arrangement.clips[track];
                    let mut fresh = clips[index].clone();
                    let (start, at) = place_clip(clips, clips[index].start, fresh.len);
                    fresh.id = id;
                    fresh.name = format!("{} {}", clips[index].name, request.verb);
                    fresh.start = start;
                    repoint_clip(&mut fresh, &rendered, request.length_change);
                    clips.insert(at, fresh);
                    self.arrangement.selected_clip = Some((track, at));
                }
                Some((track, index)) => {
                    let bpm = self.transport.bpm;
                    let clips = &mut self.arrangement.clips[track];
                    repoint_clip(&mut clips[index], &rendered, request.length_change);
                    if let Some(ratio) = request.rescale {
                        rescale_after_transpose(&mut clips[index], ratio);
                    }
                    // A CLIP PITCH change landing: remember the original
                    // file and the ratio now baked in, so the knobs can
                    // come home to zero without another render.
                    if let Some((ratio, base)) = request.transpose_apply.take()
                        && let Some(audio) = clips[index].audio.as_mut()
                    {
                        audio.transposed_from = Some(base);
                        audio.applied_ratio = ratio;
                    }
                    if request.flatten {
                        neutralise_after_flatten(&mut clips[index], &rendered);
                        clips[index].name = format!("{} flat", clips[index].name);
                    }
                    if request.ripple
                        && let Some((_, delta)) = request.length_change
                        && delta != 0
                    {
                        let rate = f64::from(rendered.sample_rate.max(1));
                        let beats = (delta as f64 / rate * bpm / 60.0) as f32;
                        ripple_lane(clips, index, beats);
                    }
                    resort(clips);
                }
                None => {
                    // A launcher slot, or nothing at all. A slot has no
                    // neighbours to ripple into; a clip that was deleted
                    // while the render ran leaves the file on disk, which
                    // is cheap and is what an undo would want back.
                    let slot = self
                        .arrangement
                        .session
                        .slots
                        .iter_mut()
                        .flatten()
                        .flatten()
                        .find(|clip| clip.id == request.clip);
                    let Some(clip) = slot else { continue };
                    repoint_clip(clip, &rendered, request.length_change);
                    if let Some(ratio) = request.rescale {
                        rescale_after_transpose(clip, ratio);
                    }
                    if request.flatten {
                        neutralise_after_flatten(clip, &rendered);
                    }
                }
            }
            self.arrangement.force_recompile = true;
            self.notice = Some(format!("{} rendered", request.verb));
        }
    }

    /// Reconcile the selected audio clip with the green-zone peak cache.
    /// One path is requested once; duplicated clips reuse the same Arc.
    pub(crate) fn pump_waveforms(&mut self) {
        while let Some(loaded) = self.waveform_service.try_result() {
            self.waveform_pending.remove(&loaded.path);
            match loaded.result {
                Ok(peaks) => {
                    self.waveform_failed.remove(&loaded.path);
                    self.waveform_cache.insert(loaded.path, peaks);
                }
                Err(error) => {
                    self.waveform_failed.insert(loaded.path);
                    self.notice = Some(error.to_string());
                }
            }
        }

        // The sample windows the editor asks for when it is zoomed past
        // what the peak pyramid can honestly draw. The editor holds no
        // handle to the service — it states a wish and this reconciles
        // it, exactly as `apply_clip_edit` does for its controls.
        while let Some(window) = self.waveform_service.try_window() {
            if let Err(error) = &window.result {
                self.notice = Some(error.to_string());
            }
            self.waveform.accept_window(window);
        }
        if let Some(key) = self.waveform.take_window_request() {
            self.waveform_service.request_window(key);
        }

        self.pump_renders();
        self.waveform
            .follow_clip(self.arrangement.active_audio_clip_id());
        // Arrangement thumbnails need every placed source, not only the
        // selected editor target. A duplicated path is inserted into
        // `waveform_pending` once and shares the finished Arc everywhere.
        // Every placed source, wherever it is placed: the launcher's slots
        // draw the same thumbnails the timeline's clips do.
        let placed = self
            .arrangement
            .clips
            .iter()
            .flatten()
            .chain(self.arrangement.session.slots.iter().flatten().flatten());
        for clip in placed {
            let Some(path) = clip.audio.as_ref().map(|audio| &audio.path) else {
                continue;
            };
            if !self.waveform_cache.contains_key(path)
                && !self.waveform_failed.contains(path)
                && self.waveform_pending.insert(path.clone())
            {
                self.waveform_service.request(path.clone());
            }
        }
    }

    // ---- recording -----------------------------------------------------
}
