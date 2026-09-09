use super::{Stage, palette};
use crate::midi_lab::{Voice, composer::*};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::stage::{
    composer::{self as controller, Control, State, Subject},
    lab::Instrument,
};
use eframe::egui::{self, FontId, Rect, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug)]
struct Regions {
    header: Rect,
    status: Rect,
    score: Rect,
    inspector: Rect,
    analysis: Rect,
}
fn regions(rect: Rect) -> Regions {
    let r = rect.shrink(10.);
    let header = Rect::from_min_max(r.min, pos2(r.right(), r.top() + 28.));
    let status = Rect::from_min_max(
        pos2(r.left(), header.bottom() + 4.),
        pos2(r.right(), header.bottom() + 25.),
    );
    let analysis = Rect::from_min_max(pos2(r.left(), r.bottom() - 43.), r.max);
    let content = Rect::from_min_max(
        pos2(r.left(), status.bottom() + 8.),
        pos2(r.right(), analysis.top() - 8.),
    );
    let split = content.left() + (content.width() * 0.56).max(content.width() / 2.);
    Regions {
        header,
        status,
        score: Rect::from_min_max(content.min, pos2(split - 5., content.bottom())),
        inspector: Rect::from_min_max(pos2(split + 5., content.top()), content.max),
        analysis,
    }
}
fn text(ui: &egui::Ui, rect: Rect, value: &str, size: f32, color: egui::Color32) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        value.to_owned(),
        FontId::proportional(size),
        color,
    );
    job.wrap.max_width = rect.width().max(1.);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    ui.painter().with_clip_rect(rect).galley(
        rect.left_center() - vec2(0., galley.size().y / 2.),
        galley,
        color,
    );
}

impl Stage {
    pub(super) fn draw_composer(&mut self, parent: &mut egui::Ui, rect: Rect, window: usize) {
        if parent.rect_contains_pointer(rect) && parent.input(|i| i.pointer.any_pressed()) {
            self.lab.focus = Some(window);
            self.lab.inside = true;
        }
        let Some(Instrument::Midi(mut state)) =
            self.lab.window(window).map(|w| w.instrument.clone())
        else {
            return;
        };
        let Some(index) = self.song.midi_labs.iter().position(|d| d.id == state.draft) else {
            return;
        };
        let mut draft = self.song.midi_labs[index].clone();
        self.composer_context(&mut state.composer, draft.destination);
        let before = draft.clone();
        let Some(composition) = draft.recipe.composition.as_mut() else {
            return;
        };
        let c = composition.as_mut();
        if state
            .played
            .is_some_and(|at| at.elapsed().as_secs_f64() >= state.preview_seconds)
        {
            state.played = None;
        }
        let input = c.input().unwrap_or_default();
        if input != state.composer.result_input {
            match render(c) {
                Ok(rendered) => {
                    state.composer.result = Some(rendered);
                    state.composer.error = None;
                }
                Err(e) => state.composer.error = Some(e),
            }
            state.composer.result_input = input;
        }
        if self.lab.focus == Some(window)
            && !parent.ctx().egui_wants_keyboard_input()
            && parent.input(|i| i.key_pressed(egui::Key::F6))
        {
            state.composer.score_focus = !state.composer.score_focus;
            state.composer.chase = true;
        }
        let r = regions(rect);
        let colors = palette::colours();
        let mut action = 0u8;
        let mut separate = false;
        let mut root = parent.new_child(
            egui::UiBuilder::new()
                .id_salt(("composer", window))
                .max_rect(rect),
        );
        root.set_clip_rect(rect);
        root.style_mut().visuals.override_text_color = Some(colors.fg);
        root.style_mut().visuals.widgets.inactive.bg_fill = colors.panel;
        root.style_mut().visuals.widgets.inactive.weak_bg_fill = colors.panel;
        root.style_mut().visuals.extreme_bg_color = colors.ground;
        root.style_mut().spacing.item_spacing = vec2(5., 4.);
        root.painter().rect_filled(rect, 0., colors.ground);
        let mut header =
            root.new_child(egui::UiBuilder::new().id_salt("header").max_rect(r.header));
        header.horizontal(|ui| {
            ui.label(
                egui::RichText::new("MIDI LAB")
                    .size(17.)
                    .color(colors.bright),
            );
            let response =
                ui.add(egui::TextEdit::singleline(&mut state.address).desired_width(44.));
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                match self.resolve_midi_tag(&state.address) {
                    Ok(destination) => {
                        draft.destination = Some(destination);
                        state.status = format!("Destination {}", state.address);
                    }
                    Err(e) => state.status = e,
                }
            }
            if ui.button("Hear").clicked() {
                action = 1;
            }
            if ui.button("Play").clicked() {
                action = 2;
            }
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("Send").color(colors.nominal),
                ))
                .clicked()
            {
                action = 3;
            }
            if ui.button("Stop").clicked() {
                state.cancel();
                state.status = "Stopped".into();
            }
            if ui.button("Find").clicked() {
                state.composer.palette = !state.composer.palette;
                state.composer.picker_index = 0;
                state.composer.palette_query.clear();
            }
            if ui.button("Undo").clicked() {
                action = 4;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(c.fingerprint().unwrap_or_default())
                        .monospace()
                        .size(10.)
                        .color(colors.dim),
                );
            });
        });
        let fields = controller::controls(&state.composer, c);
        state.composer.focus = state.composer.focus.min(fields.len().saturating_sub(1));
        let focused = if state.composer.score_focus {
            None
        } else {
            fields.get(state.composer.focus).copied()
        };
        let readout = state.composer.error.clone().unwrap_or_else(|| {
            if state.composer.score_focus {
                return format!(
                    "Score · {} · arrows navigate · Shift+arrows edit · F6 inspector",
                    state.composer.voice.label()
                );
            }
            if let Some(field) = focused {
                format!(
                    "{} · {} · Shift+arrows edit · Enter acts",
                    field.label(),
                    controller::reading(field, c, &state.composer)
                )
            } else {
                state.status.clone()
            }
        });
        text(
            &root,
            r.status,
            &if state.status.starts_with("Error:") {
                state.status.clone()
            } else {
                readout
            },
            12.,
            if state.composer.error.is_some() || state.status.starts_with("Error:") {
                colors.fault
            } else {
                colors.label
            },
        );
        root.painter().line_segment(
            [r.status.left_bottom(), r.status.right_bottom()],
            Stroke::new(1., colors.rule),
        );
        let output = state.composer.result.clone();
        let mut score = root.new_child(egui::UiBuilder::new().id_salt("score").max_rect(r.score));
        score.set_clip_rect(r.score);
        if state.composer.score_focus {
            score.painter().rect_stroke(
                r.score,
                0.,
                Stroke::new(1., colors.chassis),
                egui::StrokeKind::Inside,
            );
        }
        let playhead = state.played.map(|at| {
            output::tick_at_seconds(
                state.composer.tempo,
                &state.composer.tempo_marks,
                at.elapsed().as_secs_f64(),
            )
        });
        score_view(
            &mut score,
            r.score,
            c,
            &mut state.composer,
            output.as_ref(),
            playhead,
        );
        let mut inspector = root.new_child(
            egui::UiBuilder::new()
                .id_salt("inspector")
                .max_rect(r.inspector),
        );
        inspector.set_clip_rect(r.inspector);
        let mut message = None;
        egui::ScrollArea::vertical()
            .id_salt("inspector-scroll")
            .auto_shrink([false, false])
            .show(&mut inspector, |ui| {
                if state.composer.palette {
                    palette_picker(ui, c, &mut state.composer);
                    return;
                }
                ui.label(
                    egui::RichText::new(state.composer.subject.label().to_uppercase())
                        .size(15.)
                        .color(colors.label),
                );
                ui.add_space(6.);
                let mut displayed = 0;
                let mut advanced_heading = false;
                for (at, field) in fields.iter().copied().enumerate() {
                    if let Control::PitchClass(pc) = field {
                        if pc == 0 {
                            pitch_chips(ui, c, &mut state.composer, at);
                            displayed += 2;
                        }
                        continue;
                    }
                    if displayed >= 8 {
                        if !advanced_heading {
                            if state.composer.chase && state.composer.focus >= at {
                                state.composer.advanced = true;
                            }
                            let label = if state.composer.advanced {
                                "More controls -"
                            } else {
                                "More controls +"
                            };
                            if ui
                                .selectable_label(state.composer.advanced, label)
                                .clicked()
                            {
                                state.composer.advanced = !state.composer.advanced;
                            }
                            advanced_heading = true;
                        }
                        if !state.composer.advanced {
                            continue;
                        }
                    }
                    let old = c.clone();
                    let old_state = state.composer.clone();
                    let result = field_row(
                        ui,
                        c,
                        &mut state.composer,
                        field,
                        at,
                        focused == Some(field),
                    );
                    if field == Control::Separate && result.as_ref().is_ok_and(|r| *r) {
                        separate = true;
                    }
                    if let Err(e) = result.and_then(|_| c.validate()) {
                        *c = old;
                        state.composer = old_state;
                        message = Some(format!("Error: {e}"));
                    }
                    displayed += 1;
                }
                if matches!(state.composer.subject, Subject::Melody | Subject::Bass) {
                    ui.add_space(10.);
                    ui.label(egui::RichText::new("CONTOUR").size(11.).color(colors.label));
                    let vi = controller::voice(&state.composer).index();
                    if curve_editor(ui, "contour", &mut c.voices[vi].melody.curve) {
                        c.voices[vi].melody.contour = Contour::Drawn;
                    }
                }
                if matches!(state.composer.subject, Subject::Melody | Subject::Bass)
                    && let Some(candidate) = state.composer.candidates.get(state.composer.candidate)
                {
                    ui.add_space(8.);
                    ui.label(
                        egui::RichText::new("CANDIDATE")
                            .size(11.)
                            .color(colors.label),
                    );
                    for change in &candidate.differences {
                        ui.label(egui::RichText::new(change).size(11.).color(colors.fg));
                    }
                }
                if state.composer.subject == Subject::Rhythm {
                    let vi = controller::voice(&state.composer).index();
                    ui.label("Velocity shape");
                    if c.voices[vi].velocity_curve.is_empty() {
                        if ui.small_button("Add velocity curve").clicked() {
                            c.voices[vi].velocity_curve = vec![(0, 50), (500, 65), (1000, 50)];
                        }
                    } else {
                        curve_editor(ui, "velocity", &mut c.voices[vi].velocity_curve);
                    }
                    ui.label("Gate shape");
                    if c.voices[vi].gate_curve.is_empty() {
                        if ui.small_button("Add gate curve").clicked() {
                            c.voices[vi].gate_curve = vec![(0, 50), (500, 65), (1000, 50)];
                        }
                    } else {
                        curve_editor(ui, "gate", &mut c.voices[vi].gate_curve);
                    }
                }
                if state.composer.subject == Subject::Form {
                    ui.add_space(10.);
                    ui.label("TENSION");
                    curve_editor(ui, "tension", &mut c.tension);
                }
                if matches!(state.composer.subject, Subject::Harmony | Subject::Voicing) {
                    if let Some(h) = c.harmony.get(state.composer.chord) {
                        let readings = h
                            .material
                            .readings()
                            .iter()
                            .map(|r| r.1.clone())
                            .collect::<Vec<_>>()
                            .join(" / ");
                        ui.label(
                            egui::RichText::new(if readings.is_empty() {
                                "No additional reading".into()
                            } else {
                                readings
                            })
                            .size(11.)
                            .color(colors.dim),
                        );
                        if let Some(key) = c.key_at(h.start) {
                            ui.label(
                                egui::RichText::new(
                                    crate::theory::functional::analysis(&h.material, key)
                                        .join(" / "),
                                )
                                .size(12.)
                                .color(colors.fg),
                            );
                        }
                        if let Some(rendered) = &output
                            && let Some(v) = rendered.voicings.iter().find(|v| v.harmony == h.id)
                        {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Cost {} · {} candidates",
                                    v.cost.total(),
                                    v.candidates
                                ))
                                .size(11.)
                                .color(colors.dim),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "Motion {} · leap {} · common {} · spacing {}",
                                    v.cost.motion, v.cost.leap, v.cost.common, v.cost.spacing
                                ))
                                .size(10.)
                                .color(colors.dim),
                            );
                            if let Some((notes, cost)) = &v.runner_up {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Runner-up {notes:?} · ranked cost {cost}"
                                    ))
                                    .size(10.)
                                    .color(colors.dim),
                                );
                            }
                            let (area, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 180.),
                                egui::Sense::hover(),
                            );
                            picture(ui, area, h, v, &mut state.camera, state.draft);
                        }
                    }
                }
                if state.composer.subject == Subject::Note
                    && let Some(note) = output
                        .as_ref()
                        .and_then(|r| r.notes.iter().find(|n| Some(n.id) == state.composer.note))
                {
                    ui.separator();
                    ui.label(
                        egui::RichText::new(&note.provenance.detail)
                            .size(12.)
                            .color(colors.fg),
                    );
                    if let Some(target) = note.provenance.target {
                        ui.label(format!(
                            "Target: {}{}",
                            crate::theory::pitch_class_name(target),
                            i16::from(target) / 12 - 1
                        ));
                    }
                    if let Some(harmony) = output.as_ref().and_then(|r| {
                        r.harmony
                            .iter()
                            .find(|h| Some(h.id) == note.provenance.harmony)
                    }) {
                        ui.label(format!("Harmony: {}", harmony.material.label()));
                    }
                    if let Some(motif) = c
                        .motifs
                        .iter()
                        .find(|m| Some(m.id) == note.provenance.motif)
                    {
                        ui.label(format!("Source motif: {}", motif.name));
                    }
                    for transform in &note.provenance.transforms {
                        ui.label(egui::RichText::new(transform).size(11.).color(colors.dim));
                    }
                }
                if state.composer.subject == Subject::Recipe
                    && let Some(rendered) = &output
                    && let Some(primary) = draft.destination
                {
                    ui.separator();
                    ui.label("OUTPUT PREVIEW");
                    match output::deliveries(c, rendered, primary) {
                        Ok(deliveries) => {
                            ui.label(format!(
                                "{} delivered notes · {} clips",
                                deliveries.iter().map(|d| d.events.len()).sum::<usize>(),
                                deliveries.len()
                            ));
                            for change in deliveries.iter().flat_map(|d| &d.changes).take(32) {
                                ui.label(change);
                            }
                        }
                        Err(error) => {
                            ui.label(egui::RichText::new(error).color(colors.alert));
                        }
                    }
                }
            });
        state.composer.chase = false;
        if let Some(message) = message {
            state.status = message;
        }
        root.painter().line_segment(
            [r.analysis.left_top(), r.analysis.right_top()],
            Stroke::new(1., colors.rule),
        );
        if let Some(rendered) = &output {
            let numerals = rendered
                .harmony
                .iter()
                .take(12)
                .map(|h| {
                    c.key_at(h.start).map_or_else(
                        || h.material.label(),
                        |k| {
                            let names = crate::theory::functional::analysis(&h.material, k);
                            if names.is_empty() {
                                h.material.label()
                            } else {
                                names.join("/")
                            }
                        },
                    )
                })
                .collect::<Vec<_>>()
                .join("   ·   ");
            text(
                &root,
                Rect::from_min_size(r.analysis.min + vec2(0., 3.), vec2(r.analysis.width(), 17.)),
                &format!(
                    "{} notes · {} beats   {numerals}",
                    rendered.notes.len(),
                    f64::from(rendered.length) / 48.
                ),
                11.,
                colors.fg,
            );
            let explanation = state
                .composer
                .note
                .and_then(|id| rendered.notes.iter().find(|n| n.id == id))
                .map(|n| format!("{} · {}", n.provenance.rule, n.provenance.detail))
                .or_else(|| rendered.findings.first().map(|f| f.detail.clone()))
                .unwrap_or_else(|| {
                    format!(
                        "{} harmony spans · {} enabled voices · {} / {}",
                        rendered.harmony.len(),
                        c.voices.iter().filter(|v| v.enabled).count(),
                        c.meter.numerator,
                        c.meter.denominator
                    )
                });
            text(
                &root,
                Rect::from_min_size(
                    r.analysis.min + vec2(0., 22.),
                    vec2(r.analysis.width(), 18.),
                ),
                &explanation,
                11.,
                if rendered.findings.is_empty() {
                    colors.dim
                } else {
                    colors.alert
                },
            );
        }
        if draft != before {
            state.cancel();
            self.song.midi_labs[index] = draft;
        }
        if let Some(w) = self.lab.window_mut(window) {
            w.instrument = Instrument::Midi(state.clone());
        }
        if !parent.input(|i| i.pointer.any_down()) && !parent.ctx().egui_wants_keyboard_input() {
            self.settle();
        }
        if separate
            && let Err(reason) = self.composer_separate(window)
            && let Some(w) = self.lab.window_mut(window)
            && let Instrument::Midi(m) = &mut w.instrument
        {
            m.status = format!(
                "Cannot separate tracks: {reason:?} · check the composition and destination"
            );
        }
        let result = match action {
            1 => self.midi_hear(window, true).map(|_| None),
            2 => self.midi_hear(window, false).map(|_| None),
            3 => self.midi_send(state.draft).map(Some),
            4 => {
                let _ = self.apply(crate::ui::stage::StageIntent::Undo);
                Ok(None)
            }
            _ => Ok(None),
        };
        if let Some(w) = self.lab.window_mut(window)
            && let Instrument::Midi(m) = &mut w.instrument
        {
            match result {
                Err(e) => m.status = format!("Error: {e}"),
                Ok(Some(message)) => m.status = message,
                _ => {}
            }
            if m.job.is_some() || m.played.is_some() {
                parent
                    .ctx()
                    .request_repaint_after(std::time::Duration::from_millis(30));
            }
        }
    }
}

fn field_row(
    ui: &mut egui::Ui,
    c: &mut Composition,
    s: &mut State,
    field: Control,
    index: usize,
    focused: bool,
) -> Result<bool, String> {
    let colors = palette::colours();
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.), egui::Sense::hover());
    let value = Rect::from_min_max(pos2(row.left() + row.width() * 0.43, row.top()), row.max);
    text(
        ui,
        Rect::from_min_max(row.min, pos2(value.left() - 5., row.bottom())),
        &field.label(),
        11.,
        colors.label,
    );
    if focused {
        ui.painter().rect_stroke(
            row,
            2.,
            Stroke::new(1., colors.chassis),
            egui::StrokeKind::Inside,
        );
        if s.chase {
            ui.scroll_to_rect(row, Some(egui::Align::Center));
        }
    }
    if field.text() && s.edit == Some(field) {
        let default = controller::reading(field, c, s);
        let buffer = s.texts.entry(field).or_insert(default);
        let response = ui.put(
            value,
            egui::TextEdit::singleline(buffer)
                .font(FontId::proportional(12.))
                .id(ui.id().with(("text", field))),
        );
        if !response.has_focus() {
            response.request_focus();
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let text = buffer.clone();
            response.surrender_focus();
            controller::text_edit(c, s, field, &text)?;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            s.edit = None;
            response.surrender_focus();
        }
        return Ok(false);
    }
    let response = ui
        .interact(
            value,
            ui.id().with(("field", field)),
            if !field.action() && !field.text() && !controller::choice(field) {
                egui::Sense::click_and_drag()
            } else {
                egui::Sense::click()
            },
        )
        .affords(if field.text() {
            Affords::Write
        } else if field.action() || controller::choice(field) {
            Affords::Press
        } else {
            Affords::Sweep
        });
    if response.dragged() {
        let id = ui.id().with(("field-drag", field));
        let delta = ui
            .input(|i| {
                i.pointer
                    .interact_pos()
                    .zip(i.pointer.press_origin())
                    .map(|(a, b)| a - b)
            })
            .unwrap_or_default();
        let steps = (delta.x / 8.).trunc() as i32;
        if response.drag_started() {
            ui.ctx().data_mut(|d| d.insert_temp(id, 0i32));
        }
        let previous = ui.ctx().data(|d| d.get_temp::<i32>(id)).unwrap_or(0);
        if steps != previous {
            controller::turn(c, s, field, steps - previous)?;
            ui.ctx().data_mut(|d| d.insert_temp(id, steps));
        }
    }
    if response.hovered() || focused {
        ui.painter().line_segment(
            [
                value.right_center() - vec2(6., 3.),
                value.right_center() - vec2(3., 0.),
            ],
            Stroke::new(1., colors.dim),
        );
    }

    let reading = controller::reading(field, c, s);
    response
        .clone()
        .on_hover_text(format!("{}: {}", field.label(), reading));
    let colour = if field == Control::RemoveChord || field == Control::DeleteNote {
        colors.alert
    } else if focused {
        colors.bright
    } else {
        colors.fg
    };
    text(ui, value.shrink2(vec2(5., 0.)), &reading, 12., colour);
    if response.hovered() {
        ui.painter().rect_stroke(
            value,
            1.,
            Stroke::new(1., colors.rule),
            egui::StrokeKind::Inside,
        );
    }
    if response.clicked() {
        s.score_focus = false;
        s.focus = index;
        s.chase = true;
        if field == Control::Separate {
            return Ok(true);
        }
        if controller::choice(field) {
            s.picker = if s.picker == Some(field) {
                None
            } else {
                Some(field)
            };
            s.query.clear();
            s.picker_index = 0;
        } else if field.text() {
            s.edit = Some(field);
        } else {
            controller::act(c, s, field)?;
        }
    }
    if focused
        && !field.action()
        && !field.text()
        && !controller::choice(field)
        && response.hovered()
    {
        let delta = ui.input(|i| i.smooth_scroll_delta.y);
        if delta.abs() > 0.1 {
            controller::turn(c, s, field, if delta > 0. { 1 } else { -1 })?;
        }
    }
    if s.picker == Some(field) {
        let search = ui.add(
            egui::TextEdit::singleline(&mut s.query)
                .hint_text("Search choices")
                .desired_width(ui.available_width()),
        );
        if !search.has_focus() && s.picker_index == 0 {
            search.request_focus();
        }
        let options = controller::options(c, s, field);
        let choices = options
            .iter()
            .enumerate()
            .filter(|(_, label)| label.to_lowercase().contains(&s.query.to_lowercase()))
            .collect::<Vec<_>>();
        if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            s.picker_index = (s.picker_index + 1).min(choices.len().saturating_sub(1));
        }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            s.picker_index = s.picker_index.saturating_sub(1);
        }
        s.picker_index = s.picker_index.min(choices.len().saturating_sub(1));
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt(("picker", field))
            .max_height(160.)
            .show(ui, |ui| {
                for (row, (offset, label)) in choices.iter().enumerate() {
                    let response = ui.selectable_label(row == s.picker_index, *label);
                    if response.clicked() {
                        selected = Some(*offset);
                    }
                    if row == s.picker_index
                        && ui.input(|i| {
                            i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp)
                        })
                    {
                        ui.scroll_to_rect(response.rect, None);
                    }
                }
            });
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            selected = choices.get(s.picker_index).map(|(offset, _)| *offset);
        }
        if let Some(offset) = selected {
            for _ in 0..offset {
                controller::turn(c, s, field, 1)?;
            }
            s.picker = None;
            s.query.clear();
            search.surrender_focus();
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            s.picker = None;
            s.query.clear();
            search.surrender_focus();
        }
    }
    Ok(false)
}

fn score_view(
    ui: &mut egui::Ui,
    rect: Rect,
    c: &mut Composition,
    s: &mut State,
    rendered: Option<&Rendered>,
    playhead: Option<f64>,
) {
    let colors = palette::colours();
    if ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.any_pressed()) {
        s.score_focus = true;
    }
    let length = rendered.map_or(c.length, |r| r.length);
    let bar = c.meter.bar().unwrap_or(192);
    let display = if s.zoom == 0 {
        length
    } else {
        s.zoom.min(length)
    }
    .max(1);
    s.scroll = s.scroll.min(length.saturating_sub(display));
    let mut top = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("score-header")
            .max_rect(Rect::from_min_size(rect.min, vec2(rect.width(), 27.))),
    );
    top.horizontal(|ui| {
        ui.label(egui::RichText::new("SCORE").size(14.).color(colors.label));
        ui.label(
            egui::RichText::new(c.key.as_ref().map_or("No key".into(), |k| k.label()))
                .size(11.)
                .color(colors.fg),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Fit").clicked() {
                s.zoom = 0;
                s.scroll = 0;
            }
            if ui.small_button("+").clicked() {
                s.zoom = (display / 2).max(bar);
            }
            if ui.small_button("-").clicked() {
                s.zoom = (display * 2).min(length);
            }
            if ui.small_button("›").clicked() {
                s.scroll = (s.scroll + bar).min(length.saturating_sub(display));
            }
            if ui.small_button("‹").clicked() {
                s.scroll = s.scroll.saturating_sub(bar);
            }
        });
    });
    let left = rect.left() + 68.;
    let width = (rect.right() - left - 4.).max(1.);
    let scale = width / display as f32;
    let x = |tick: u32| left + (tick as f32 - s.scroll as f32) * scale;
    let ruler = rect.top() + 40.;
    for tick in (s.scroll / bar * bar..=s.scroll + display).step_by(bar as usize) {
        let at = x(tick);
        ui.painter().line_segment(
            [pos2(at, ruler), pos2(at, rect.bottom())],
            Stroke::new(1., colors.rule),
        );
        text(
            ui,
            Rect::from_min_size(pos2(at + 3., ruler - 15.), vec2(40., 14.)),
            &(tick / bar + 1).to_string(),
            10.,
            colors.dim,
        );
    }
    let mut section_y = ruler + 3.;
    if !c.sections.is_empty() {
        text(
            ui,
            Rect::from_min_size(pos2(rect.left(), section_y), vec2(64., 18.)),
            "Sections",
            11.,
            colors.label,
        );
        let mut offset = 0;
        let entries = if c.form.is_empty() {
            c.sections
                .iter()
                .enumerate()
                .map(|(i, s)| (i, s.start, s.length))
                .collect::<Vec<_>>()
        } else {
            c.form
                .iter()
                .filter_map(|id| {
                    c.sections.iter().position(|s| s.id == *id).map(|i| {
                        let span = (i, offset, c.sections[i].length);
                        offset += c.sections[i].length;
                        span
                    })
                })
                .collect()
        };
        for (i, start, length) in entries {
            let area = Rect::from_min_size(
                pos2(x(start), section_y),
                vec2((length as f32 * scale).max(5.), 18.),
            );
            ui.painter().rect_stroke(
                area,
                2.,
                Stroke::new(
                    1.,
                    if s.subject == Subject::Form && s.section == i {
                        colors.chassis
                    } else {
                        colors.rule
                    },
                ),
                egui::StrokeKind::Inside,
            );
            text(
                ui,
                area.shrink2(vec2(4., 0.)),
                &c.sections[i].name,
                11.,
                colors.fg,
            );
            if ui
                .interact(
                    area,
                    ui.id().with(("section", c.sections[i].id, start)),
                    egui::Sense::click(),
                )
                .affords(Affords::Press)
                .clicked()
            {
                s.subject = Subject::Form;
                s.section = i;
                s.focus = 1;
                s.chase = true;
            }
        }
        section_y += 21.;
    }
    for modulation in &c.modulations {
        if modulation.start >= s.scroll && modulation.start <= s.scroll + display {
            let at = x(modulation.start);
            ui.painter().line_segment(
                [pos2(at, ruler - 8.), pos2(at, ruler + 2.)],
                Stroke::new(2., colors.nominal),
            );
            let area = Rect::from_min_size(pos2(at + 3., ruler - 14.), vec2(85., 14.));
            text(ui, area, &modulation.key.label(), 10., colors.nominal);
        }
    }
    let htop = section_y;

    text(
        ui,
        Rect::from_min_size(pos2(rect.left(), htop), vec2(64., 26.)),
        "Harmony",
        11.,
        colors.label,
    );
    let hs = if c.form.is_empty() {
        c.harmony.clone()
    } else {
        rendered.map_or_else(|| c.harmony.clone(), |r| r.harmony.clone())
    };
    for (i, h) in hs.iter().enumerate() {
        if h.start + h.length < s.scroll || h.start > s.scroll + display {
            continue;
        }
        let r = Rect::from_min_size(
            pos2(x(h.start), htop),
            vec2((h.length as f32 * scale).max(5.), 26.),
        );
        ui.painter().rect_filled(
            r,
            2.,
            if s.chord == i {
                colors.select
            } else {
                colors.panel
            },
        );
        text(
            ui,
            r.shrink2(vec2(4., 0.)),
            &h.material.label(),
            11.,
            if s.chord == i {
                colors.bright
            } else {
                colors.fg
            },
        );
        let response = super::midi_lab::bar(
            ui,
            ui.id().with(("harmony", h.id)),
            r,
            h.start as usize,
            h.length as usize,
            0,
            scale,
            12,
            c.length as usize,
            0.,
        );
        if response.clicked {
            if c.form.is_empty() {
                s.chord = i;
                s.subject = Subject::Harmony;
            } else {
                s.subject = Subject::Form;
            }
            s.focus = 1;
            s.texts.clear();
            s.chase = true;
        }
        if c.form.is_empty()
            && let Some((start, length, _)) = response.edit
        {
            let mut next = c.clone();
            next.harmony[i].start = start as u32;
            next.harmony[i].length = length as u32;
            if next.validate().is_ok() {
                *c = next;
            }
        }
    }
    let lanes = htop + 34.;
    let lane_height = if rect.height() < 320. {
        15.
    } else if rect.height() < 400. {
        18.
    } else {
        29.
    };
    for voice in Voice::ALL {
        let y = lanes + voice.index() as f32 * lane_height;
        let label = Rect::from_min_size(pos2(rect.left(), y), vec2(64., lane_height - 2.));
        let response = ui
            .interact(
                label,
                ui.id().with(("lane", voice.index())),
                egui::Sense::click(),
            )
            .affords(Affords::Press);
        text(
            ui,
            label,
            voice.label(),
            11.,
            if c.voices[voice.index()].enabled {
                colors.nominal
            } else {
                colors.dim
            },
        );
        if response.clicked() {
            s.voice = voice;
            s.subject = match voice {
                Voice::Bass => Subject::Bass,
                Voice::Melody | Voice::Arp => Subject::Melody,
                Voice::Counterpoint => Subject::Counterpoint,
                _ => Subject::Rhythm,
            };
            s.focus = 0;
            s.chase = true;
        }
        if let Some(rendered) = rendered {
            for note in rendered.notes.iter().filter(|n| {
                n.voice == voice && n.start + n.length >= s.scroll && n.start <= s.scroll + display
            }) {
                let area = Rect::from_min_size(
                    pos2(x(note.start), y + 2.),
                    vec2(
                        (note.length as f32 * scale).max(2.),
                        (lane_height - 10.).clamp(3., 13.),
                    ),
                );
                ui.painter().rect_filled(
                    area,
                    1.,
                    if s.voice == voice {
                        colors.nominal
                    } else {
                        colors.rule
                    },
                );
            }
        }
        if c.form.is_empty() && c.voices[voice.index()].enabled {
            if let Ok(gates) = rhythm::pulses(c, voice) {
                for gate in &gates {
                    if gate.start + gate.length < s.scroll || gate.start > s.scroll + display {
                        continue;
                    }
                    let area = Rect::from_min_size(
                        pos2(x(gate.start), y + lane_height - 6.),
                        vec2((gate.length as f32 * scale).max(6.), 5.),
                    );
                    let response = super::midi_lab::bar(
                        ui,
                        ui.id().with(("rhythm-gate", voice.index(), gate.id)),
                        area,
                        gate.start as usize,
                        gate.length as usize,
                        0,
                        scale,
                        12,
                        c.length as usize,
                        0.,
                    );
                    if response.clicked {
                        s.voice = voice;
                        s.subject = Subject::Rhythm;
                        s.focus = 1;
                        s.chase = true;
                    }
                    if response.delete || response.edit.is_some() {
                        let mut custom = gates.clone();
                        if response.delete {
                            custom.retain(|p| p.id != gate.id);
                        }
                        if let Some((start, length, _)) = response.edit {
                            if let Some(p) = custom.iter_mut().find(|p| p.id == gate.id) {
                                p.start = start as u32;
                                p.length = length as u32;
                            }
                        }
                        if custom.len() <= 2048 {
                            let v = &mut c.voices[voice.index()];
                            v.rhythm.kind = RhythmKind::Custom;
                            v.rhythm.custom_length = c.length;
                            v.rhythm.custom = custom;
                            v.rhythm.offsets.clear();
                            v.rhythm.accents.clear();
                            v.rhythm.rotation = 0;
                            if voice == Voice::Bass
                                && matches!(v.bass.role, BassRole::Walking | BassRole::Sub)
                            {
                                v.bass.role = BassRole::Riff;
                            }
                        }
                    }
                }
            }
        }
    }
    let roll = Rect::from_min_max(
        pos2(left, lanes + 5. * lane_height + 24.),
        rect.max - vec2(4., 5.),
    );
    if roll.height() < 32. {
        return;
    }
    text(
        ui,
        Rect::from_min_size(pos2(rect.left(), roll.top() - 22.), vec2(rect.width(), 20.)),
        &format!(
            "{} · select a note to inspect its decision",
            s.voice.label()
        ),
        11.,
        colors.label,
    );
    let selected = rendered
        .map(|r| {
            r.notes
                .iter()
                .filter(|n| n.voice == s.voice)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let low = selected
        .iter()
        .map(|n| n.pitch)
        .min()
        .unwrap_or(c.voices[s.voice.index()].low)
        .saturating_sub(2);
    let high = selected
        .iter()
        .map(|n| n.pitch)
        .max()
        .unwrap_or(c.voices[s.voice.index()].high)
        .saturating_add(2)
        .min(127);
    let row = roll.height() / f32::from(high - low + 1);
    for pitch in low..=high {
        let y = roll.bottom() - f32::from(pitch - low + 1) * row;
        let line = Rect::from_min_size(pos2(roll.left(), y), vec2(roll.width(), row));
        if [1, 3, 6, 8, 10].contains(&(pitch % 12)) {
            ui.painter().rect_filled(line, 0., colors.panel);
        }
        if pitch % 12 == 0 {
            text(
                ui,
                Rect::from_min_size(pos2(rect.left() + 5., y), vec2(55., row.max(12.))),
                &format!("C{}", i16::from(pitch) / 12 - 1),
                10.,
                colors.dim,
            );
        }
    }
    // Background is allocated first; individual bodies and edges own drags.
    let background = ui
        .interact(roll, ui.id().with("roll-background"), egui::Sense::click())
        .affords(Affords::Draw);
    if background.double_clicked()
        && let Some(p) = background.interact_pointer_pos()
    {
        let start = (((p.x - left) / scale + s.scroll as f32) / 12.)
            .round()
            .max(0.) as u32
            * 12;
        let pitch = (f32::from(low) + (roll.bottom() - p.y) / row)
            .floor()
            .clamp(0., 127.) as u8;
        if start < length {
            let id = c.mint();
            let note = NoteEvent {
                id,
                voice: s.voice,
                member: None,
                pitch,
                start,
                length: 24.min(length - start),
                velocity: 90,
                provenance: Provenance::new(
                    "manual.note",
                    c.harmony_at(start).map(|h| h.id),
                    "Written in the score",
                ),
            };
            c.overrides.push(Override {
                id,
                instance: !c.form.is_empty(),
                inserted: Some(note),
                ..Override::default()
            });
            c.voices[s.voice.index()].enabled = true;
            s.note = Some(id);
        }
    }
    for note in selected
        .iter()
        .filter(|n| n.end() >= s.scroll && n.start <= s.scroll + display)
    {
        let r = Rect::from_min_size(
            pos2(
                x(note.start),
                roll.bottom() - f32::from(note.pitch - low + 1) * row,
            ),
            vec2((note.length as f32 * scale).max(6.), row.max(3.) - 1.),
        );
        let selected = s.note == Some(note.id);
        ui.painter().rect_filled(
            r,
            1.,
            if selected {
                colors.chassis
            } else {
                colors.nominal
            },
        );
        let response = super::midi_lab::bar(
            ui,
            ui.id().with(("note", note.id)),
            r,
            note.start as usize,
            note.length as usize,
            note.pitch,
            scale,
            12,
            length as usize,
            row,
        );
        if response.clicked {
            s.note = Some(note.id);
            s.subject = Subject::Note;
            s.focus = 1;
            s.chase = true;
        }
        if response.delete {
            c.remove_note(note.id);
        }
        if let Some((start, length, pitch)) = response.edit {
            let mut edit = c
                .overrides
                .iter()
                .find(|o| o.id == note.id)
                .cloned()
                .unwrap_or(Override {
                    id: note.id,
                    instance: !c.form.is_empty(),
                    ..Override::default()
                });
            edit.pitch = Some(pitch);
            edit.start = Some(start as u32);
            edit.length = Some(length as u32);
            c.overrides.retain(|o| o.id != note.id);
            c.overrides.push(edit);
            s.note = Some(note.id);
        }
    }
    if let Some(tick) = playhead {
        let tick = (tick % f64::from(length.max(1))) as u32;
        if tick >= s.scroll && tick <= s.scroll + display {
            let x = x(tick);
            ui.painter().line_segment(
                [pos2(x, ruler), pos2(x, rect.bottom())],
                Stroke::new(1.5, colors.nominal),
            );
        }
    }
}

fn curve_editor(ui: &mut egui::Ui, id: &str, points: &mut Vec<(u16, i16)>) -> bool {
    let colors = palette::colours();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 85.), egui::Sense::hover());
    let area = rect.shrink(8.);
    ui.painter().rect_filled(rect, 3., colors.ground);
    let mut changed = false;
    let at = |point: (u16, i16)| {
        pos2(
            area.left() + f32::from(point.0) / 1000. * area.width(),
            area.bottom() - f32::from(point.1) / 100. * area.height(),
        )
    };
    let mut ordered = points.clone();
    ordered.sort_by_key(|p| p.0);
    for pair in ordered.windows(2) {
        ui.painter()
            .line_segment([at(pair[0]), at(pair[1])], Stroke::new(1.5, colors.fg));
    }
    let background = ui
        .interact(rect, ui.id().with((id, "background")), egui::Sense::click())
        .affords(Affords::Draw);
    if background.double_clicked()
        && points.len() < 128
        && let Some(pos) = background.interact_pointer_pos()
    {
        points.push((
            ((pos.x - area.left()) / area.width() * 1000.)
                .round()
                .clamp(0., 1000.) as u16,
            ((area.bottom() - pos.y) / area.height() * 100.)
                .round()
                .clamp(0., 100.) as i16,
        ));
        changed = true;
    }
    let mut remove = None;
    for (i, point) in points.iter_mut().enumerate() {
        let pos = at(*point);
        let target = ui.id().with((id, i));
        let response = ui
            .interact(
                Rect::from_center_size(pos, vec2(14., 14.)),
                target,
                egui::Sense::click_and_drag(),
            )
            .affords(Affords::Steer);
        ui.painter().circle_filled(
            pos,
            4.,
            if response.hovered() || response.dragged() {
                colors.bright
            } else {
                colors.nominal
            },
        );
        if response.secondary_clicked() {
            remove = Some(i);
        }
        response
            .clone()
            .on_hover_text(format!("{}% of phrase · {}%", point.0 / 10, point.1));
        if response.drag_started() {
            ui.ctx().data_mut(|d| d.insert_temp(target, *point));
        }
        if response.dragged()
            && let Some(origin) = ui.ctx().data(|d| d.get_temp::<(u16, i16)>(target))
        {
            let delta = ui
                .input(|i| {
                    i.pointer
                        .interact_pos()
                        .zip(i.pointer.press_origin())
                        .map(|(a, b)| a - b)
                })
                .unwrap_or(response.drag_delta());
            point.0 = (f32::from(origin.0) + delta.x / area.width() * 1000.)
                .round()
                .clamp(0., 1000.) as u16;
            point.1 = (f32::from(origin.1) - delta.y / area.height() * 100.)
                .round()
                .clamp(0., 100.) as i16;
            changed = true;
        }
    }
    if let Some(i) = remove {
        points.remove(i);
        changed = true;
    }
    changed
}

fn picture(
    ui: &mut egui::Ui,
    rect: Rect,
    h: &HarmonySpan,
    decision: &VoicingDecision,
    camera: &mut [f32; 3],
    draft: u64,
) {
    let colors = palette::colours();
    let mut toolbar = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("camera")
            .max_rect(Rect::from_min_size(rect.min, vec2(rect.width(), 24.))),
    );
    toolbar.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Voiced pitches")
                .size(11.)
                .color(colors.label),
        );
        if ui.small_button("Top").clicked() {
            *camera = [0., 1.45, 4.];
        }
        if ui.small_button("Side").clicked() {
            *camera = [-0.6, 0.1, 4.];
        }
        if ui.small_button("3D").clicked() {
            *camera = [-0.6, 0.35, 3.5];
        }
    });
    let viewport = Rect::from_min_max(rect.min + vec2(0., 27.), rect.max - vec2(0., 22.));
    if viewport.height() < 1. {
        return;
    }
    let tones = decision
        .notes
        .iter()
        .map(|(id, p)| {
            let m = h
                .material
                .members
                .iter()
                .find(|m| m.id == *id && m.pc == *p % 12);
            let degree = m.and_then(|m| m.degree).unwrap_or(0);
            crate::theory::harmony::Tone {
                pitch: *p,
                degree,
                label: crate::theory::material::note_name(
                    m.map_or(crate::theory::pitch_class_name(*p), |m| m.spelling.as_str()),
                    *p,
                ),
                extension: degree >= 9,
            }
        })
        .collect::<Vec<_>>();
    if tones.is_empty() {
        return;
    }
    let span = f32::from(
        tones
            .iter()
            .map(|n| n.pitch)
            .max()
            .unwrap_or(0)
            .saturating_sub(tones.iter().map(|n| n.pitch).min().unwrap_or(0)),
    ) * 0.12;
    let mut view = *camera;
    view[2] = view[2].max((span + 0.35) * 1.6);
    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        viewport,
        crate::ui::kiln::Scene {
            id: usize::MAX - draft as usize,
            chord: Some(tones.clone()),
            patch: crate::kiln::Patch::default(),
            animation: None,
            time: None,
            standing: 0,
            camera: view,
            background: egui::Rgba::from(colors.ground).to_array(),
            size: [viewport.width(), viewport.height()],
        },
    ));
    let names = tones
        .iter()
        .map(|n| n.label.clone())
        .collect::<Vec<_>>()
        .join(" · ");
    text(
        ui,
        Rect::from_min_size(rect.left_bottom() - vec2(0., 20.), vec2(rect.width(), 20.)),
        &names,
        11.,
        colors.fg,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::device::probe;
    #[test]
    fn numeric_drag_accumulates_motion_and_resets_for_the_next_gesture() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(0., 0.), vec2(320., 140.));
        let mut c = Composition::default();
        let mut s = State::default();
        let initial = c.voices[0].velocity;
        for (delta, expected) in [(40., initial + 5), (-32., initial + 1)] {
            probe::run(
                &ctx,
                rect,
                &probe::drag_path(pos2(200., 12.), pos2(200. + delta, 12.), 8),
                |ui| {
                    field_row(ui, &mut c, &mut s, Control::Velocity, 0, true).unwrap();
                },
            );
            assert_eq!(c.voices[0].velocity, expected);
        }
    }
    #[test]
    fn score_note_body_and_release_have_independent_drag_targets() {
        for resize in [false, true] {
            let ctx = egui::Context::default();
            let rect = Rect::from_min_size(pos2(0., 0.), vec2(700., 650.));
            let mut c = Composition::default();
            c.length = 192;
            c.harmony.truncate(1);
            c.harmony[0].material = crate::theory::material::Material::parse("notes:C4").unwrap();
            let mut s = State::default();
            let before = render(&c).unwrap();
            let note = &before.notes[0];
            let scale = 628. / 192.;
            let row = (645. - 246.) / 5.;
            let x = if resize {
                68. + note.length as f32 * scale - 1.
            } else {
                68. + 30. * scale
            };
            let from = pos2(x, 645. - 2.5 * row);
            let to = from + vec2(if resize { -24. * scale } else { 12. * scale }, 0.);
            probe::run(&ctx, rect, &probe::drag_path(from, to, 5), |ui| {
                let r = render(&c).unwrap();
                score_view(ui, rect, &mut c, &mut s, Some(&r), None);
            });
            let after = render(&c).unwrap();
            let changed = &after.notes[0];
            assert_eq!((changed.id, changed.pitch), (note.id, note.pitch));
            if resize {
                assert_eq!(changed.start, note.start);
                assert!(changed.length < note.length);
            } else {
                assert_eq!(changed.length, note.length);
                assert_eq!(changed.start, note.start + 12);
            }
        }
    }
    #[test]
    fn rhythm_gate_drag_keeps_the_original_cell_and_changes_its_release() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(0., 0.), vec2(700., 650.));
        let mut c = Composition::default();
        let mut s = State::default();
        let before = rhythm::pulses(&c, Voice::Chords).unwrap();
        let scale = 628. / c.length as f32;
        let from = pos2(68. + before[0].length as f32 * scale - 1., 102.);
        probe::run(
            &ctx,
            rect,
            &probe::drag_path(from, from - vec2(24. * scale, 0.), 5),
            |ui| {
                let r = render(&c).unwrap();
                score_view(ui, rect, &mut c, &mut s, Some(&r), None);
            },
        );
        let after = rhythm::pulses(&c, Voice::Chords).unwrap();
        assert_eq!(after[0].id, before[0].id);
        assert_eq!(after[0].start, before[0].start);
        assert!(after[0].length < before[0].length);
        assert_eq!(after[1..], before[1..]);
    }
    #[test]
    fn five_regions_keep_score_width_and_do_not_overlap() {
        for size in [vec2(720., 480.), vec2(1280., 800.), vec2(1920., 1080.)] {
            let r = regions(Rect::from_min_size(pos2(0., 0.), size));
            assert!(r.score.width() >= (size.x - 30.) / 2.);
            assert!(r.header.bottom() < r.status.top());
            assert!(r.status.bottom() < r.score.top());
            assert!(r.score.right() < r.inspector.left());
            assert!(r.score.bottom() < r.analysis.top());
        }
    }
    #[test]
    fn curve_drag_retains_its_handle_when_crossing_another() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(0., 0.), vec2(320., 140.));
        let mut points = vec![(0, 20), (500, 90), (1000, 35)];
        let original = points.clone();
        probe::run(
            &ctx,
            rect,
            &probe::drag_path(pos2(8., 61.6), pos2(260., 61.6), 5),
            |ui| {
                curve_editor(ui, "test", &mut points);
            },
        );
        assert_ne!(points[0], original[0]);
        assert_eq!(points[1], original[1]);
        assert_eq!(points[2], original[2]);
    }
}

fn pitch_chips(ui: &mut egui::Ui, c: &mut Composition, s: &mut State, index: usize) {
    let colors = palette::colours();
    ui.label(
        egui::RichText::new("PITCH CLASSES")
            .size(11.)
            .color(colors.label),
    );
    let width = (ui.available_width() - 25.) / 6.;
    for row in 0..2 {
        ui.horizontal(|ui| {
            for col in 0..6 {
                let pc = (row * 6 + col) as u8;
                let included = c
                    .harmony
                    .get(s.chord)
                    .is_some_and(|h| h.material.mask() & (1 << pc) != 0);
                let focus = s.focus == index + usize::from(pc);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(width, 24.), egui::Sense::click());
                ui.painter().rect_filled(
                    rect,
                    3.,
                    if included {
                        colors.select
                    } else {
                        colors.ground
                    },
                );
                ui.painter().rect_stroke(
                    rect,
                    3.,
                    Stroke::new(
                        if focus { 2. } else { 1. },
                        if focus { colors.chassis } else { colors.rule },
                    ),
                    egui::StrokeKind::Inside,
                );
                text(
                    ui,
                    rect.shrink2(vec2(5., 0.)),
                    crate::theory::pitch_class_name(pc),
                    12.,
                    if included { colors.nominal } else { colors.dim },
                );
                if response.clicked() {
                    s.focus = index + usize::from(pc);
                    let _ = controller::turn(c, s, Control::PitchClass(pc), 1);
                }
            }
        });
    }
}

fn palette_picker(ui: &mut egui::Ui, c: &Composition, s: &mut State) {
    ui.label("FIND A CONTROL");
    let search = ui.add(
        egui::TextEdit::singleline(&mut s.palette_query)
            .hint_text("Chord, rhythm, bass, motif, form…"),
    );
    if !search.has_focus() {
        search.request_focus();
    }
    let query = s.palette_query.to_lowercase();
    let mut commands = Vec::new();
    for subject in Subject::ALL {
        let mut context = s.clone();
        context.subject = subject;
        for (index, field) in controller::controls(&context, c).into_iter().enumerate() {
            let name = format!("{} · {}", subject.label(), field.label());
            if field != Control::Subject && name.to_lowercase().contains(&query) {
                commands.push((subject, index, name));
            }
        }
    }
    s.picker_index = s.picker_index.min(commands.len().saturating_sub(1));
    if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
        s.picker_index = (s.picker_index + 1).min(commands.len().saturating_sub(1));
    }
    if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
        s.picker_index = s.picker_index.saturating_sub(1);
    }
    let mut select = None;
    for (row, (subject, index, name)) in commands.iter().enumerate() {
        let r = ui.selectable_label(row == s.picker_index, name);
        if r.clicked() || row == s.picker_index && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            select = Some((*subject, *index));
        }
        if row == s.picker_index
            && ui
                .input(|i| i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp))
        {
            ui.scroll_to_rect(r.rect, None);
        }
    }
    if let Some((subject, index)) = select {
        s.subject = subject;
        s.focus = index;
        s.chase = true;
        s.score_focus = false;
        s.palette = false;
        search.surrender_focus();
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        s.palette = false;
        search.surrender_focus();
    }
}
