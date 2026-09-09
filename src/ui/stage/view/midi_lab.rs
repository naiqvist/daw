//! MIDI Lab: concrete pitches above a shared tick ruler. Native egui hit
//! targets own every bar and resize edge; the chord itself is a wgpu scene.
use super::{Stage, palette};
use crate::ui::affordance::{Afford, Affords};
use crate::ui::stage::{
    lab::Instrument,
    midi_lab::{Field, MidiLab, focused, motion_label},
};
use crate::{
    midi_lab::{self, Event, Gate, Recipe, Rhythm, Voice},
    theory::harmony::{self, HarmonicStyle, Layout},
};
use eframe::egui::{self, Align2, FontId, Rect, Stroke, pos2, vec2};

impl Stage {
    pub(super) fn draw_midi_lab(&mut self, parent: &mut egui::Ui, rect: Rect, window: usize) {
        if parent.rect_contains_pointer(rect) && parent.input(|i| i.pointer.any_pressed()) {
            self.lab.focus = Some(window);
            self.lab.inside = true;
        }
        let Some(w) = self.lab.window(window) else {
            return;
        };
        let Instrument::Midi(mut state) = w.instrument.clone() else {
            return;
        };
        let Some(mut draft) = self
            .song
            .midi_labs
            .iter()
            .find(|d| d.id == state.draft)
            .cloned()
        else {
            return;
        };
        let before = draft.clone();
        let cursor = Cursor {
            at: focused(&draft.recipe, &state),
            chase: std::mem::take(&mut state.chase),
        };
        let c = palette::colours();
        let mut action = 0;
        let mut address = false;
        let mut ui = parent.new_child(
            egui::UiBuilder::new()
                .id_salt(("midi", window))
                .max_rect(rect.shrink(12.)),
        );
        ui.set_clip_rect(rect);
        ui.style_mut().visuals.override_text_color = Some(c.fg);
        ui.style_mut().visuals.widgets.inactive.bg_fill = c.panel;
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = c.panel;
        ui.style_mut().visuals.extreme_bg_color = c.ground;
        ui.style_mut().visuals.text_edit_bg_color = Some(c.ground);
        ui.style_mut().visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., c.rule);
        ui.style_mut().spacing.item_spacing = vec2(7., 7.);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("MIDI LAB")
                    .strong()
                    .size(18.)
                    .color(c.bright),
            );
            ui.separator();
            ui.label("Clip");
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.address)
                    .desired_width(48.)
                    .hint_text("a1"),
            );
            address = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ring(ui, response.rect, cursor, Field::Clip);
            let target = ui.button("Target");
            ring(ui, target.rect, cursor, Field::Target);
            address |= target.clicked();
            let hear = ui.button("Hear chord · H");
            ring(ui, hear.rect, cursor, Field::Hear);
            if hear.clicked() {
                action = 1;
            }
            let play = ui.button("Play clip · P");
            ring(ui, play.rect, cursor, Field::Play);
            if play.clicked() {
                action = 2;
            }
            let stop = ui.button("Stop · Shift+P");
            ring(ui, stop.rect, cursor, Field::Stop);
            if stop.clicked() {
                state.cancel();
            }
            let send = ui
                .button("Send to clip · S")
                .on_hover_text("Replace the destination clip's notes in one undoable edit");
            ring(ui, send.rect, cursor, Field::Send);
            if send.clicked() {
                action = 3;
            }
            if ui.button("Undo").clicked() {
                action = 4;
            }
        });
        ui.label(egui::RichText::new(&state.status).size(12.).color(c.dim));
        egui::ScrollArea::vertical().id_salt("midi-content").show(&mut ui,|ui|{
            let entry_width=(ui.available_width()-175.).max(120.);
            ui.horizontal(|ui|{
                ui.label("Progression");
                ui.add(egui::TextEdit::singleline(&mut state.progression).desired_width(entry_width).hint_text("Cmaj7:4 Am7:2 Dm7:1 G7:1"));
                if ui.button("Apply").clicked(){match draft.recipe.progression(&state.progression){Ok(())=>{state.chord=0;state.status="Progression applied · durations are in beats".into();},Err(e)=>state.status=e}}
            });
            let width=ui.available_width();let middle_h=if rect.height()>670.{270.}else{240.};
            let (middle,_)=ui.allocate_exact_size(vec2(width,middle_h),egui::Sense::hover());
            let split=if width>850.{0.48}else{0.42};
            let left=Rect::from_min_max(middle.min,pos2(middle.left()+width*split-8.,middle.bottom()));
            let right=Rect::from_min_max(pos2(left.right()+16.,middle.top()),middle.max);
            let generated=midi_lab::generate(&draft.recipe);
            geometry(ui,left,&draft.recipe,&state,generated.as_ref().ok());
            let mut controls=ui.new_child(egui::UiBuilder::new().id_salt("harmony-controls").max_rect(right));
            egui::ScrollArea::vertical().id_salt("voicing-scroll").show(&mut controls,|ui|voicing_controls(ui,&mut draft.recipe,&mut state,cursor));
            ui.add_space(10.);
            ui.horizontal_wrapped(|ui|{
                for voice in Voice::ALL {
                    let label = if draft.recipe.voices[voice.index()].enabled {
                        format!("● {}", voice.label())
                    } else {
                        voice.label().into()
                    };
                    let chip = ui.selectable_label(state.voice == voice, label);
                    if state.voice == voice {
                        ring(ui, chip.rect, cursor, Field::Voice);
                    }
                    if chip.clicked() {
                        state.voice = voice;
                        state.note = None;
                    }
                }
                ui.separator();
                let rhythm = ui.selectable_value(&mut state.roll, false, "Rhythm");
                let notes = ui.selectable_value(&mut state.roll, true, "Notes");
                ring(ui, rhythm.rect.union(notes.rect), cursor, Field::View);
            });
            voice_controls(ui,&mut draft.recipe,&mut state,cursor);
            let generated=midi_lab::generate(&draft.recipe);
            let timeline_height=if state.roll{190.}else{164.};
            timeline(ui,&mut draft.recipe,&mut state,generated.as_ref().ok(),timeline_height);
            if state.roll {note_controls(ui,&mut draft.recipe,&mut state,generated.as_ref().ok());}
            match &generated {
                Ok(g)=>{
                    ui.horizontal_wrapped(|ui|{ui.colored_label(c.nominal,format!("{} notes · {} beats",g.notes.len(),draft.recipe.length as f32/48.));ui.label("Angle = pitch class · height = register · orange = root · blue = extensions");});
                    for warning in &g.warnings {ui.colored_label(c.alert,warning);}
                },Err(e)=>{ui.colored_label(c.fault,e);}
            }
            ui.label(egui::RichText::new("Keys: arrows walk every control · Shift+← → changes it · Shift+↑↓ by more · Tab next row · Enter acts").small().color(c.dim));
            ui.label(egui::RichText::new("Pointer: drag spans to move · drag their right edge to resize · double-click a gap to place · right-click a gate or note to delete").small().color(c.dim));
        });
        address |= matches!(action, 1..=3);
        if address {
            match self.resolve_midi_tag(&state.address) {
                Ok(d) => {
                    draft.destination = Some(d);
                    state.status = format!("Destination locked to {}", self.song.tag_of(d.pattern));
                }
                Err(e) => {
                    state.status = e;
                    action = 0;
                }
            }
        }
        if draft != before {
            state.cancel();
            if let Some(d) = self.song.midi_labs.iter_mut().find(|d| d.id == draft.id) {
                *d = draft;
            }
            // One history entry for a complete pointer gesture; fields commit
            // on focus loss or buttons. Never snapshot every pixel of a drag.
            if !parent.input(|i| i.pointer.any_down()) && !parent.ctx().egui_wants_keyboard_input()
            {
                self.settle();
            }
        } else if !parent.input(|i| i.pointer.any_down())
            && !parent.ctx().egui_wants_keyboard_input()
        {
            self.settle();
        }
        if state.job.is_some() || state.played.is_some() {
            parent
                .ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        }
        if let Some(w) = self.lab.window_mut(window) {
            w.instrument = Instrument::Midi(state.clone());
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
                Err(e) => m.status = e,
                Ok(Some(s)) => m.status = s,
                _ => {}
            }
        }
    }
}

/// Where the keyboard cursor is, handed down to every control that draws
/// itself so the one under it can say so.
#[derive(Clone, Copy)]
struct Cursor {
    at: Option<Field>,
    /// The cursor has just moved and the page should scroll to it once.
    chase: bool,
}

impl Cursor {
    fn on(self, field: Field) -> bool {
        self.at == Some(field)
    }

    /// For a control the page cursor does not reach. A note's own pitch and
    /// velocity belong to whichever note the roll has selected, and the roll
    /// is selected with the pointer, so those two keep the pointer's ring —
    /// which is none.
    fn nowhere() -> Self {
        Self {
            at: None,
            chase: false,
        }
    }
}

/// Ring the control the keyboard is standing on, and bring it into view the
/// first frame after it moves.
///
/// The ring is drawn around the widget the pointer uses, not beside it:
/// there is one page here and two ways to drive it, and the keyboard has to
/// land on the same things the hand does.
fn ring(ui: &egui::Ui, rect: Rect, cursor: Cursor, field: Field) {
    if !cursor.on(field) {
        return;
    }
    let c = palette::colours();
    ui.painter().rect_stroke(
        rect.expand(3.),
        3.,
        Stroke::new(1.5, c.chassis),
        egui::StrokeKind::Outside,
    );
    if cursor.chase {
        ui.scroll_to_rect(rect.expand(28.), None);
    }
}

fn voicing_controls(ui: &mut egui::Ui, recipe: &mut Recipe, state: &mut MidiLab, cursor: Cursor) {
    let c = palette::colours();
    state.chord = state.chord.min(recipe.harmony.len().saturating_sub(1));
    ui.horizontal_wrapped(|ui| {
        for (i, h) in recipe.harmony.iter().enumerate() {
            let chip = ui.selectable_label(i == state.chord, &h.symbol);
            if chip.clicked() {
                state.chord = i;
            }
            if i == state.chord {
                ring(ui, chip.rect, cursor, Field::Chord);
            }
        }
    });
    let Some(h) = recipe.harmony.get_mut(state.chord) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label("Chord");
        let symbol = ui.add(egui::TextEdit::singleline(&mut h.symbol).desired_width(100.));
        // Root and Quality are the keyboard's halves of this one field.
        ring(ui, symbol.rect, cursor, Field::Root);
        ring(ui, symbol.rect, cursor, Field::Quality);
        egui::ComboBox::from_id_salt("quality-palette")
            .selected_text("Palette")
            .show_ui(ui, |ui| {
                for root in [
                    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
                ] {
                    ui.menu_button(root, |ui| {
                        for quality in [
                            "maj7", "m7", "7", "m7b5", "dim7", "sus4", "6", "m9", "maj9", "7b9",
                            "7#9", "7#11",
                        ] {
                            if ui.button(format!("{root}{quality}")).clicked() {
                                h.symbol = format!("{root}{quality}");
                                ui.close();
                            }
                        }
                    });
                }
            });
    });
    ui.horizontal_wrapped(|ui| {
        let layout_box = egui::ComboBox::from_id_salt("layout")
            .selected_text(h.voicing.layout.label())
            .show_ui(ui, |ui| {
                for layout in Layout::ALL {
                    ui.selectable_value(&mut h.voicing.layout, layout, layout.label());
                }
            });
        ring(ui, layout_box.response.rect, cursor, Field::Layout);
        stepper(
            ui,
            "Inv",
            &mut h.voicing.inversion,
            0,
            9,
            cursor,
            Field::Inversion,
        );
        signed_stepper(
            ui,
            "Oct",
            &mut h.voicing.octave,
            -1,
            8,
            cursor,
            Field::Octave,
        );
    });
    ui.horizontal_wrapped(|ui| {
        let lead = ui.checkbox(&mut h.voicing.lead, "Voice leading");
        ring(ui, lead.rect, cursor, Field::Lead);
        ui.label("Range");
        stepper(
            ui,
            "Low",
            &mut h.voicing.low,
            0,
            127,
            cursor,
            Field::RangeLow,
        );
        stepper(
            ui,
            "High",
            &mut h.voicing.high,
            0,
            127,
            cursor,
            Field::RangeHigh,
        );
    });
    if let Ok(chord) = harmony::parse(&h.symbol) {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(c.dim, "Members");
            let mut members = chord.members.clone();
            members.extend(h.voicing.added.iter().copied());
            members.sort_by_key(|m| m.degree);
            members.dedup_by_key(|m| m.degree);
            for m in members {
                let on = !h.voicing.omitted.contains(&m.degree);
                let chip = ui
                    .selectable_label(on, harmony::degree_label(m))
                    .on_hover_text("Toggle this member in the chord voicing");
                ring(ui, chip.rect, cursor, Field::Member(m.degree));
                if chip.clicked() {
                    if on {
                        h.voicing.omitted.push(m.degree);
                    } else {
                        h.voicing.omitted.retain(|d| *d != m.degree);
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(c.dim, "Colours");
            for m in harmony::available(&chord)
                .into_iter()
                .filter(|m| m.degree >= 9)
            {
                let on = (h.voicing.added.contains(&m) || chord.members.contains(&m))
                    && !h.voicing.omitted.contains(&m.degree);
                let chip = ui.selectable_label(on, harmony::degree_label(m));
                ring(ui, chip.rect, cursor, Field::Colour(m.degree));
                if chip.clicked() {
                    if on {
                        h.voicing.added.retain(|n| *n != m);
                        if chord.members.contains(&m) {
                            h.voicing.omitted.push(m.degree);
                        }
                    } else {
                        if !chord.members.contains(&m) {
                            h.voicing.added.push(m);
                        }
                        h.voicing.omitted.retain(|d| *d != m.degree);
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Spread a degree");
            for degree in [1, 3, 5, 7, 9, 11, 13] {
                let menu = ui.menu_button(degree.to_string(), |ui| {
                    for octave in -2..=2 {
                        if ui.button(format!("{octave:+} octave")).clicked() {
                            h.voicing.offsets.retain(|(d, _)| *d != degree);
                            if octave != 0 {
                                h.voicing.offsets.push((degree, octave));
                            }
                            ui.close();
                        }
                    }
                    let doubled = h.voicing.doubled.contains(&degree);
                    if ui.selectable_label(doubled, "Double + octave").clicked() {
                        if doubled {
                            h.voicing.doubled.retain(|d| *d != degree);
                        } else {
                            h.voicing.doubled.push(degree);
                        }
                        ui.close();
                    }
                });
                ring(ui, menu.response.rect, cursor, Field::Spread(degree));
            }
        });
    }
    ui.horizontal_wrapped(|ui| {
        let rules = egui::ComboBox::from_id_salt("rules")
            .selected_text(recipe.style.label())
            .show_ui(ui, |ui| {
                for style in [HarmonicStyle::Strict, HarmonicStyle::Chromatic] {
                    ui.selectable_value(&mut recipe.style, style, style.label());
                }
            });
        ring(ui, rules.response.rect, cursor, Field::Style);
        let remove = ui.button("Remove chord");
        ring(ui, remove.rect, cursor, Field::Remove);
        if remove.clicked() && recipe.harmony.len() > 1 {
            recipe.harmony.remove(state.chord);
            state.chord = state.chord.saturating_sub(1);
        }
        if ui.button("Camera ↶").clicked() {
            state.camera[0] -= 0.25;
        }
        if ui.button("↷").clicked() {
            state.camera[0] += 0.25;
        }
        if ui.button("Top").clicked() {
            state.camera[1] = 1.45;
        }
        if ui.button("3D").clicked() {
            state.camera = [-0.6, 0.35, 3.5];
        }
    });
}

fn stepper(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u8,
    min: u8,
    max: u8,
    cursor: Cursor,
    field: Field,
) {
    let group = ui
        .horizontal(|ui| {
            ui.label(format!("{label} {value}"));
            if ui.small_button("-").clicked() {
                *value = value.saturating_sub(1).max(min);
            }
            if ui.small_button("+").clicked() {
                *value = value.saturating_add(1).min(max);
            }
        })
        .response
        .rect;
    ring(ui, group, cursor, field);
}
fn signed_stepper(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut i16,
    min: i16,
    max: i16,
    cursor: Cursor,
    field: Field,
) {
    let group = ui
        .horizontal(|ui| {
            ui.label(format!("{label} {value}"));
            if ui.small_button("-").clicked() {
                *value = (*value - 1).max(min);
            }
            if ui.small_button("+").clicked() {
                *value = (*value + 1).min(max);
            }
        })
        .response
        .rect;
    ring(ui, group, cursor, field);
}

fn voice_controls(ui: &mut egui::Ui, r: &mut Recipe, s: &mut MidiLab, cursor: Cursor) {
    let v = &mut r.voices[s.voice.index()];
    ui.horizontal_wrapped(|ui| {
        let on = ui.checkbox(&mut v.enabled, "On");
        ring(ui, on.rect, cursor, Field::On);
        let rhythm_box = egui::ComboBox::from_id_salt("rhythm")
            .selected_text(v.rhythm.label())
            .show_ui(ui, |ui| {
                for rhythm in Rhythm::ALL {
                    ui.selectable_value(&mut v.rhythm, rhythm, rhythm.label());
                }
            });
        ring(ui, rhythm_box.response.rect, cursor, Field::Rhythm);
        // The same table the keyboard's Motion control reads, so the two
        // can never come to disagree about what a mode is called.
        let modes = [0u8, 1, 2, 3].map(|mode| motion_label(s.voice, mode));
        if matches!(s.voice, Voice::Arp | Voice::Bass | Voice::Melody) {
            let motion = egui::ComboBox::from_id_salt("motion")
                .selected_text(modes[v.motion.min(3) as usize])
                .show_ui(ui, |ui| {
                    for (i, label) in modes.iter().enumerate() {
                        ui.selectable_value(&mut v.motion, i as u8, *label);
                    }
                });
            ring(ui, motion.response.rect, cursor, Field::Motion);
        }
        let again = ui.add_enabled(
            s.voice != Voice::Chords,
            egui::Button::new("Regenerate voice"),
        );
        ring(ui, again.rect, cursor, Field::Regenerate);
        if again.clicked() {
            v.seed = v.seed.wrapping_add(1);
        }
        stepper(ui, "Gate %", &mut v.gate, 5, 100, cursor, Field::Gate);
        stepper(ui, "Swing", &mut v.swing, 50, 75, cursor, Field::Swing);
        stepper(ui, "Vel", &mut v.velocity, 1, 127, cursor, Field::Velocity);
    });
    ui.horizontal_wrapped(|ui| {
        if s.voice != Voice::Chords {
            stepper(ui, "Low", &mut v.low, 0, 127, cursor, Field::VoiceLow);
            stepper(ui, "High", &mut v.high, 0, 127, cursor, Field::VoiceHigh);
        }
        if v.rhythm == Rhythm::Euclidean {
            stepper(ui, "Hits", &mut v.pulses, 0, 32, cursor, Field::Hits);
            stepper(ui, "Steps", &mut v.steps, 1, 32, cursor, Field::Steps);
            stepper(ui, "Rotate", &mut v.rotation, 0, 31, cursor, Field::Rotate);
        }
        ui.label("Snap");
        let grid = egui::ComboBox::from_id_salt("grid")
            .selected_text(format!("{} ticks", s.snap))
            .show_ui(ui, |ui| {
                for (tick, label) in [
                    (1, "Free · 1 tick"),
                    (8, "1/16 triplet"),
                    (12, "1/16"),
                    (24, "1/8"),
                ] {
                    ui.selectable_value(&mut s.snap, tick, label);
                }
            });
        ring(ui, grid.response.rect, cursor, Field::Snap);
        let length = ui.label("Length");
        ring(ui, length.rect, cursor, Field::Length);
        if ui.small_button("- beat").clicked()
            && r.length > 48
            && r.harmony
                .iter()
                .all(|h| h.start + h.length <= r.length - 48)
        {
            r.length -= 48;
        }
        if ui.small_button("+ beat").clicked() {
            r.length = (r.length + 48).min(crate::sequencing::DEFAULT_PATTERN_TICKS);
        }
    });
}

fn geometry(
    ui: &mut egui::Ui,
    rect: Rect,
    r: &Recipe,
    s: &MidiLab,
    g: Option<&midi_lab::Generated>,
) {
    let c = palette::colours();
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 4., c.ground);
    let Some(h) = r.harmony.get(s.chord) else {
        return;
    };
    p.text(
        rect.min + vec2(10., 8.),
        Align2::LEFT_TOP,
        format!("{}   /   {}", h.symbol, h.voicing.layout.label()),
        FontId::proportional(18.),
        c.bright,
    );
    let Some(tones) = g.and_then(|g| g.voicings.iter().find(|(id, _)| *id == h.id).map(|v| &v.1))
    else {
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Choose compatible chord members",
            FontId::proportional(12.),
            c.dim,
        );
        return;
    };
    let viewport = Rect::from_min_max(rect.min + vec2(0., 30.), rect.max - vec2(0., 28.));
    let mut camera = s.camera;
    let span = tones
        .first()
        .zip(tones.last())
        .map_or(0., |(a, b)| f32::from(b.pitch - a.pitch) * 0.12);
    camera[2] = camera[2].max((span + 0.35) * 1.6);
    p.add(egui_wgpu::Callback::new_paint_callback(
        viewport,
        crate::ui::kiln::Scene {
            id: usize::MAX - s.draft as usize,
            chord: Some(tones.clone()),
            patch: crate::kiln::Patch::default(),
            animation: None,
            time: None,
            standing: 0,
            camera,
            background: egui::Rgba::from(c.ground).to_array(),
            size: [viewport.width(), viewport.height()],
        },
    ));
    let aspect = viewport.width() / viewport.height();
    let distance = camera[2] * (0.9 / aspect).max(1.);
    let eye = glam::Vec3::new(
        camera[0].sin() * camera[1].cos(),
        camera[1].sin(),
        camera[0].cos() * camera[1].cos(),
    ) * distance;
    let vp = glam::camera::rh::proj::directx::perspective(48f32.to_radians(), aspect, 0.05, 40.)
        * glam::camera::rh::view::look_at_mat4(eye, glam::Vec3::new(0., 0.1, 0.), glam::Vec3::Y);
    for (tone, point) in tones.iter().zip(crate::ui::kiln::chord_positions(tones)) {
        let q = vp * point.extend(1.);
        let at = pos2(
            viewport.center().x + q.x / q.w * viewport.width() * 0.5 + 9.,
            viewport.center().y - q.y / q.w * viewport.height() * 0.5 - 9.,
        );
        p.text(
            at,
            Align2::LEFT_BOTTOM,
            &tone.label,
            FontId::proportional(12.),
            if tone.degree == 1 { c.alert } else { c.bright },
        );
    }
    p.text(
        rect.left_bottom() + vec2(10., -7.),
        Align2::LEFT_BOTTOM,
        tones
            .iter()
            .map(|n| n.label.as_str())
            .collect::<Vec<_>>()
            .join("   "),
        FontId::proportional(12.),
        c.fg,
    );
}

#[derive(Clone, Copy)]
struct DragOrigin {
    start: usize,
    length: usize,
    pitch: u8,
    scale: f32,
    row: f32,
}
#[derive(Default)]
struct BarResponse {
    clicked: bool,
    delete: bool,
    edit: Option<(usize, usize, u8)>,
}
/// Both regions have independent stable ids. Captured origin survives overlap
/// with a neighbour and travel outside the entire timeline.
fn bar(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: Rect,
    start: usize,
    length: usize,
    pitch: u8,
    scale: f32,
    snap: usize,
    max: usize,
    row: f32,
) -> BarResponse {
    let mut out = BarResponse::default();
    let edge = Rect::from_min_max(
        pos2((rect.right() - 5.).max(rect.left() + 2.), rect.top()),
        rect.max,
    );
    let body = Rect::from_min_max(rect.min, pos2(edge.left(), rect.bottom()));
    for (resize, area) in [(false, body), (true, edge)] {
        let target = id.with(resize);
        let response = ui
            .interact(area, target, egui::Sense::click_and_drag())
            .affords(if resize {
                Affords::Sweep
            } else {
                Affords::Carry
            });
        if response.hovered() || response.dragged() {
            ui.painter().rect_stroke(
                area,
                1.,
                Stroke::new(1., palette::colours().bright),
                egui::StrokeKind::Inside,
            );
        }
        out.clicked |= response.clicked();
        out.delete |= response.secondary_clicked();
        if response.drag_started() {
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    target,
                    DragOrigin {
                        start,
                        length,
                        pitch,
                        scale,
                        row,
                    },
                )
            });
        }
        if response.dragged() {
            if let Some(origin) = ui.ctx().data(|d| d.get_temp::<DragOrigin>(target)) {
                let delta = ui
                    .input(|i| {
                        i.pointer
                            .interact_pos()
                            .zip(i.pointer.press_origin())
                            .map(|(now, press)| now - press)
                    })
                    .unwrap_or(response.drag_delta());
                let snap = if ui.input(|i| i.modifiers.alt) {
                    1
                } else {
                    snap.max(1)
                };
                let ticks = (delta.x / origin.scale / snap as f32).round() as i64 * snap as i64;
                if resize {
                    out.edit = Some((
                        origin.start,
                        (origin.length as i64 + ticks)
                            .clamp(1, max.saturating_sub(origin.start).max(1) as i64)
                            as usize,
                        origin.pitch,
                    ));
                } else {
                    let start = (origin.start as i64 + ticks)
                        .clamp(0, max.saturating_sub(origin.length) as i64)
                        as usize;
                    let pitch = (i16::from(origin.pitch)
                        - (delta.y / origin.row.max(1.)).round() as i16
                            * if origin.row > 0. { 1 } else { 0 })
                    .clamp(0, 127) as u8;
                    out.edit = Some((start, origin.length, pitch));
                }
            }
        }
    }
    out
}

fn timeline(
    ui: &mut egui::Ui,
    r: &mut Recipe,
    s: &mut MidiLab,
    g: Option<&midi_lab::Generated>,
    height: f32,
) {
    let c = palette::colours();
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), height + 52.),
        egui::Sense::hover(),
    );
    let label_w = 104.;
    let field = Rect::from_min_max(rect.min + vec2(label_w, 20.), rect.max);
    let scale = field.width() / r.length as f32;
    let p = ui.painter().clone();
    p.rect_filled(rect, 3., c.ground);
    for tick in (0..=r.length).step_by(12) {
        let x = field.left() + tick as f32 * scale;
        let beat = tick % 48 == 0;
        p.line_segment(
            [pos2(x, field.top()), pos2(x, field.bottom())],
            Stroke::new(
                if beat { 1. } else { 0.5 },
                if beat { c.rule } else { c.edge },
            ),
        );
        if beat {
            p.text(
                pos2(x + 3., rect.top() + 2.),
                Align2::LEFT_TOP,
                format!("{}", tick / 48 + 1),
                FontId::proportional(10.),
                c.dim,
            );
        }
    }
    let harmony_y = field.top();
    p.text(
        pos2(rect.left() + 5., harmony_y + 13.),
        Align2::LEFT_CENTER,
        "Harmony",
        FontId::proportional(12.),
        c.fg,
    );
    let harmony_row = Rect::from_min_max(
        pos2(field.left(), harmony_y),
        pos2(field.right(), harmony_y + 28.),
    );
    let mut over = false;
    let original = r.harmony.clone();
    for (i, h) in r.harmony.iter_mut().enumerate() {
        let b = Rect::from_min_size(
            pos2(field.left() + h.start as f32 * scale, harmony_y + 2.),
            vec2((h.length as f32 * scale - 2.).max(9.), 24.),
        );
        p.rect_filled(b, 3., if i == s.chord { c.select } else { c.panel });
        p.rect_stroke(
            b,
            3.,
            Stroke::new(1., if i == s.chord { c.alert } else { c.chassis }),
            egui::StrokeKind::Inside,
        );
        p.with_clip_rect(b).text(
            b.left_center() + vec2(5., 0.),
            Align2::LEFT_CENTER,
            &h.symbol,
            FontId::proportional(12.),
            c.bright,
        );
        over |= ui.rect_contains_pointer(b);
        let response = bar(
            ui,
            ui.id().with(("harmony", h.id)),
            b,
            h.start,
            h.length,
            0,
            scale,
            s.snap,
            r.length,
            0.,
        );
        if response.clicked {
            s.chord = i;
        }
        if let Some((start, length, _)) = response.edit {
            let floor = original
                .iter()
                .filter(|x| x.id != h.id && x.start < h.start)
                .map(|x| x.start + x.length)
                .max()
                .unwrap_or(0);
            let ceiling = original
                .iter()
                .filter(|x| x.id != h.id && x.start > h.start)
                .map(|x| x.start)
                .min()
                .unwrap_or(r.length);
            h.start = start.clamp(floor, ceiling.saturating_sub(length).max(floor));
            h.length = length.min(ceiling - h.start).max(1);
            s.chord = i;
        }
    }
    if !over
        && ui
            .interact(
                harmony_row,
                ui.id().with("harmony-gap"),
                egui::Sense::click(),
            )
            .affords(Affords::Draw)
            .double_clicked()
    {
        if let Some(at) = ui.input(|i| i.pointer.interact_pos()) {
            let tick = (((at.x - field.left()) / scale / s.snap as f32).floor() as usize * s.snap)
                .min(r.length - 1);
            if r.harmony_at(tick).is_none() {
                let end = r
                    .harmony
                    .iter()
                    .filter(|h| h.start > tick)
                    .map(|h| h.start)
                    .min()
                    .unwrap_or(r.length);
                let id = r.mint();
                r.harmony.push(midi_lab::Harmony {
                    id,
                    symbol: "Cmaj7".into(),
                    start: tick,
                    length: 48.min(end - tick),
                    voicing: Default::default(),
                });
                s.chord = r.harmony.len() - 1;
            }
        }
    }
    let lower = Rect::from_min_max(field.min + vec2(0., 34.), field.max);
    if s.roll {
        piano(ui, &p, rect, lower, scale, r, s, g);
    } else {
        for voice in Voice::ALL {
            let row = Rect::from_min_size(
                pos2(lower.left(), lower.top() + voice.index() as f32 * 26.),
                vec2(lower.width(), 23.),
            );
            let selected = voice == s.voice;
            let color = if selected { c.chassis } else { c.dim };
            let label = Rect::from_min_max(
                pos2(rect.left(), row.top()),
                pos2(field.left() - 3., row.bottom()),
            );
            if ui
                .put(
                    label,
                    egui::Button::new(egui::RichText::new(voice.label()).size(11.))
                        .selected(selected),
                )
                .clicked()
            {
                s.voice = voice;
            }
            let gates = midi_lab::generate::gates(r, voice);
            let mut occupied = false;
            for gate in &gates {
                if gate.start >= r.length {
                    continue;
                }
                let b = Rect::from_min_size(
                    pos2(row.left() + gate.start as f32 * scale, row.top() + 3.),
                    vec2(
                        (gate.length.min(r.length - gate.start) as f32 * scale - 1.).max(8.),
                        17.,
                    ),
                );
                occupied |= ui.rect_contains_pointer(b);
                p.rect_filled(
                    b,
                    2.,
                    if r.voices[voice.index()].enabled {
                        color.gamma_multiply(0.30)
                    } else {
                        c.panel
                    },
                );
                p.line_segment([b.left_top(), b.left_bottom()], Stroke::new(2., color));
                let response = bar(
                    ui,
                    ui.id().with(("gate", voice.index(), gate.id)),
                    b,
                    gate.start,
                    gate.length,
                    0,
                    scale,
                    s.snap,
                    r.length,
                    0.,
                );
                if response.clicked {
                    s.voice = voice;
                }
                if response.edit.is_some() || response.delete {
                    let v = &mut r.voices[voice.index()];
                    if v.rhythm != Rhythm::Custom {
                        v.custom = gates.clone();
                        v.rhythm = Rhythm::Custom;
                    }
                    if response.delete {
                        v.custom.retain(|x| x.id != gate.id);
                    } else if let Some((start, length, _)) = response.edit
                        && let Some(g) = v.custom.iter_mut().find(|g| g.id == gate.id)
                    {
                        g.start = start;
                        g.length = length;
                    }
                }
            }
            if !occupied
                && ui
                    .interact(
                        row,
                        ui.id().with(("gate-gap", voice.index())),
                        egui::Sense::click(),
                    )
                    .affords(Affords::Draw)
                    .double_clicked()
            {
                if let Some(at) = ui.input(|i| i.pointer.interact_pos()) {
                    let start = (((at.x - row.left()) / scale / s.snap as f32).floor() as usize
                        * s.snap)
                        .min(r.length - 1);
                    let id = r.mint();
                    let v = &mut r.voices[voice.index()];
                    if v.rhythm != Rhythm::Custom {
                        v.custom = gates;
                        v.rhythm = Rhythm::Custom;
                    }
                    v.enabled = true;
                    v.custom.push(Gate {
                        id,
                        start,
                        length: 12.min(r.length - start),
                        velocity: v.velocity,
                    });
                    s.voice = voice;
                }
            }
        }
    }
    if let Some(time) = s.played {
        let tick = time.elapsed().as_secs_f32() * s.bpm as f32 / 60. * 48.;
        if tick < r.length as f32 {
            let x = field.left() + tick * scale;
            p.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.5, c.alert),
            );
        } else {
            s.played = None;
        }
    }
}

fn piano(
    ui: &mut egui::Ui,
    p: &egui::Painter,
    outer: Rect,
    rect: Rect,
    scale: f32,
    r: &mut Recipe,
    s: &mut MidiLab,
    g: Option<&midi_lab::Generated>,
) {
    let c = palette::colours();
    let notes = g
        .map(|g| {
            g.notes
                .iter()
                .filter(|n| n.voice == s.voice)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            r.pinned
                .iter()
                .filter(|n| n.voice == s.voice)
                .cloned()
                .collect()
        });
    let low = notes
        .iter()
        .map(|n| n.pitch)
        .min()
        .unwrap_or(48)
        .saturating_sub(2);
    let high = notes
        .iter()
        .map(|n| n.pitch)
        .max()
        .unwrap_or(72)
        .saturating_add(2)
        .min(127);
    let range_id = ui.id().with("pitch-range");
    let (low, high) = if ui.input(|i| i.pointer.any_down()) {
        ui.ctx()
            .data(|d| d.get_temp::<(u8, u8)>(range_id))
            .unwrap_or((low, high))
    } else {
        ui.ctx().data_mut(|d| d.insert_temp(range_id, (low, high)));
        (low, high)
    };
    let row = rect.height() / f32::from(high - low + 1);
    for pitch in low..=high {
        let y = rect.top() + f32::from(high - pitch) * row;
        if [1, 3, 6, 8, 10].contains(&(pitch % 12)) {
            p.rect_filled(
                Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), row)),
                0.,
                c.panel.gamma_multiply(0.4),
            );
        }
        if pitch % 12 == 0 {
            p.text(
                pos2(outer.left() + 5., y),
                Align2::LEFT_TOP,
                format!("C{}", i16::from(pitch) / 12 - 1),
                FontId::proportional(10.),
                c.dim,
            );
        }
    }
    let mut occupied = false;
    for n in notes {
        let b = Rect::from_min_size(
            pos2(
                rect.left() + n.start as f32 * scale,
                rect.top() + f32::from(i16::from(high) - i16::from(n.pitch)) * row,
            ),
            vec2((n.length as f32 * scale).max(9.), row.max(5.) - 1.),
        );
        occupied |= ui.rect_contains_pointer(b);
        let pinned = r.pinned.iter().any(|p| p.id == n.id);
        p.rect_filled(
            b,
            1.,
            if s.note == Some(n.id) {
                c.alert
            } else if pinned {
                c.nominal
            } else {
                c.chassis
            },
        );
        let response = bar(
            ui,
            ui.id().with(("note", n.id)),
            b,
            n.start,
            n.length,
            n.pitch,
            scale,
            s.snap,
            r.length,
            row,
        );
        if response.clicked {
            s.note = Some(n.id);
        }
        if response.delete {
            r.delete_note(n.id);
        } else if let Some((start, length, pitch)) = response.edit {
            s.note = Some(n.id);
            r.pin(Event {
                start,
                length,
                pitch,
                ..n
            });
        }
    }
    if !occupied
        && ui
            .interact(rect, ui.id().with("roll-gap"), egui::Sense::click())
            .affords(Affords::Draw)
            .double_clicked()
    {
        if let Some(at) = ui.input(|i| i.pointer.interact_pos()) {
            let start = (((at.x - rect.left()) / scale / s.snap as f32).floor() as usize * s.snap)
                .min(r.length - 1);
            let pitch = high
                .saturating_sub(((at.y - rect.top()) / row) as u8)
                .max(low);
            let id = r.mint();
            r.pin(Event {
                id,
                voice: s.voice,
                pitch,
                start,
                length: 12.min(r.length - start),
                velocity: 96,
            });
            s.note = Some(id);
            r.voices[s.voice.index()].enabled = true;
        }
    }
}
fn note_controls(
    ui: &mut egui::Ui,
    r: &mut Recipe,
    s: &mut MidiLab,
    g: Option<&midi_lab::Generated>,
) {
    let Some(id) = s.note else {
        return;
    };
    let Some(mut n) = r
        .pinned
        .iter()
        .find(|n| n.id == id)
        .cloned()
        .or_else(|| g.and_then(|g| g.notes.iter().find(|n| n.id == id)).cloned())
    else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "Note {} · tick {} · {} ticks",
            n.pitch, n.start, n.length
        ));
        let old = n.clone();
        stepper(
            ui,
            "Pitch",
            &mut n.pitch,
            0,
            127,
            Cursor::nowhere(),
            Field::Velocity,
        );
        stepper(
            ui,
            "Velocity",
            &mut n.velocity,
            1,
            127,
            Cursor::nowhere(),
            Field::Velocity,
        );
        if ui.small_button("Earlier").clicked() {
            n.start = n.start.saturating_sub(s.snap);
        }
        if ui.small_button("Later").clicked() {
            n.start = (n.start + s.snap).min(r.length.saturating_sub(n.length));
        }
        if ui.small_button("Shorter").clicked() {
            n.length = n.length.saturating_sub(s.snap).max(1);
        }
        if ui.small_button("Longer").clicked() {
            n.length = (n.length + s.snap).min(r.length.saturating_sub(n.start));
        }
        if old != n {
            r.pin(n.clone());
        }
        if ui.button("Pin").clicked() {
            r.pin(n.clone());
        }
        if ui.button("Unpin").clicked() {
            r.pinned.retain(|n| n.id != id);
        }
        if ui.button("Delete note").clicked() {
            r.delete_note(id);
            s.note = None;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::device::probe;
    #[test]
    fn dragging_a_rhythm_gate_leaves_harmonic_rhythm_untouched() {
        let ctx = egui::Context::default();
        let frame = Rect::from_min_size(pos2(0., 0.), vec2(800., 300.));
        let mut recipe = Recipe::default();
        recipe.voices[0].rhythm = Rhythm::Eighth;
        let harmony = recipe.harmony.clone();
        let mut state = MidiLab::new(1, "a0".into());
        let scale = (800. - 104.) / 384.;
        let from = pos2(112., 65.);
        probe::run(
            &ctx,
            frame,
            &probe::drag_path(from, from + vec2(scale * 24., 0.), 4),
            |ui| {
                let g = midi_lab::generate(&recipe).unwrap();
                timeline(ui, &mut recipe, &mut state, Some(&g), 164.);
            },
        );
        assert_eq!(recipe.harmony, harmony);
        assert_eq!(recipe.voices[0].rhythm, Rhythm::Custom);
        assert_eq!(recipe.voices[0].custom[0].start, 24);
    }
    #[test]
    fn harmony_resize_keeps_its_neighbour_and_voice_rhythm() {
        let ctx = egui::Context::default();
        let frame = Rect::from_min_size(pos2(0., 0.), vec2(800., 300.));
        let mut recipe = Recipe::default();
        recipe.voices[0].rhythm = Rhythm::Eighth;
        let voices = recipe.voices.clone();
        let neighbour = recipe.harmony[1].clone();
        let mut state = MidiLab::new(1, "a0".into());
        let scale = (800. - 104.) / 384.;
        let from = pos2(104. + 192. * scale - 4., 34.);
        probe::run(
            &ctx,
            frame,
            &probe::drag_path(from, from - vec2(scale * 48., 0.), 4),
            |ui| {
                let g = midi_lab::generate(&recipe).unwrap();
                timeline(ui, &mut recipe, &mut state, Some(&g), 164.);
            },
        );
        assert_eq!(recipe.harmony[0].length, 144);
        assert_eq!(recipe.harmony[1], neighbour);
        assert_eq!(recipe.voices, voices);
    }
    #[test]
    fn bar_drag_and_resize_keep_the_original_target_past_neighbours() {
        for resize in [false, true] {
            let ctx = egui::Context::default();
            let frame = Rect::from_min_size(pos2(0., 0.), vec2(400., 180.));
            let mut a = (12usize, 24usize, 60u8);
            let b = (60usize, 24usize, 64u8);
            let from = pos2(
                if resize {
                    a.0 as f32 * 3. + a.1 as f32 * 3. - 2.
                } else {
                    a.0 as f32 * 3. + 10.
                },
                45.,
            );
            let path = probe::drag_path(from, from + vec2(180., 20.), 5);
            probe::run(&ctx, frame, &path, |ui| {
                let ar =
                    Rect::from_min_size(pos2(a.0 as f32 * 3., 40.), vec2(a.1 as f32 * 3., 20.));
                let response = bar(ui, egui::Id::new("a"), ar, a.0, a.1, a.2, 3., 12, 128, 10.);
                if let Some(next) = response.edit {
                    a = next;
                }
                let br =
                    Rect::from_min_size(pos2(b.0 as f32 * 3., 40.), vec2(b.1 as f32 * 3., 20.));
                assert!(
                    bar(ui, egui::Id::new("b"), br, b.0, b.1, b.2, 3., 12, 128, 10.)
                        .edit
                        .is_none()
                );
            });
            if resize {
                assert_eq!(a, (12, 84, 60));
            } else {
                assert_eq!(a, (72, 24, 58));
            }
        }
    }
}
