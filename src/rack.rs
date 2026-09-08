//! The device region: the rack of cards for the selected track, and the
//! modulation strip beside it.
//!
//! Lifted out of `main.rs` whole. The strip takes everything it needs
//! through `ModStrip`, gathered by the caller, precisely so it never
//! fights the rack for the track borrow — which is what let the two come
//! across together without untangling anything.
use super::*;

/// What the device region produced this frame, kept apart by which device
/// made it: the two cards share a parameter NUMBERING but not a node, and
/// mixing them up would send a reverb's mix to a synth's gain.
/// Everything the MOD strip needs, gathered by the caller so the strip
/// itself never fights the rack's track borrow.
pub(crate) struct ModStrip<'a> {
    /// The selected track, if any — where new wires land.
    pub(crate) track: Option<usize>,
    /// All track names, for follower tiles and the follower verb.
    pub(crate) track_names: &'a [String],
    /// What the selected track can be wired TO, gathered by the caller for
    /// the same reason the names are: enumerating it needs the track, and
    /// the cards already hold it.
    pub(crate) targets: Vec<TargetEntry>,
    pub(crate) modulators: &'a mut Vec<Modulator>,
    pub(crate) wires: &'a mut Vec<ModWire>,
    pub(crate) next_id: &'a mut u64,
    pub(crate) registry: &'a ParameterRegistry,
    /// This frame's live values by modulator id — the animation.
    pub(crate) values: &'a HashMap<u64, f32>,
    /// The playhead in beats: what an LFO's phase is a function of.
    pub(crate) beat: f32,
    /// The app's monotonic clock: what a FREE LFO's phase is a function of.
    pub(crate) seconds: f32,
    /// Each wire's chain output this frame, in target units.
    pub(crate) outputs: &'a HashMap<u64, f32>,
    /// Each wire's recent outputs, normalized to its target's range.
    pub(crate) scopes: &'a HashMap<u64, std::collections::VecDeque<f32>>,
    /// Which wire's row is unfolded into its editor.
    pub(crate) expanded: &'a mut Option<u64>,
}

/// The modulation strip at the end of the rack: source tiles on the left,
/// this track's wires as rows on the right.
///
/// Deliberately QUIET. Each tile's animation is one dot riding a static
/// curve (or one thin level bar); a wire is a text row with a centre-zero
/// depth slider and a faint tick showing the live contribution. No cables
/// are drawn anywhere — at rest a mixing decision reads better as a
/// sentence than as a wire, and the animated graph overlay is a later,
/// opt-in view.
pub(crate) fn mod_strip(
    ui: &mut egui::Ui,
    theme: &Theme,
    strip: ModStrip<'_>,
    collapsed: &mut bool,
) {
    const TILE_W: f32 = 152.0;
    const TILE_H: f32 = 88.0;
    const ROW_H: f32 = 20.0;
    let font = egui::FontId::proportional(9.0);
    // The strip scrolls vertically once cards and wires outgrow the panel:
    // nothing is ever hidden, it is just further down.
    egui::ScrollArea::vertical()
        .id_salt("mod_strip_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("MOD").small().color(theme.text_muted));
                    if ui
                        .small_button("»")
                        .on_hover_text("collapse the modulation strip")
                        .clicked()
                    {
                        *collapsed = true;
                    }
                    if ui.small_button("+ lfo").clicked() {
                        let id = *strip.next_id;
                        *strip.next_id += 1;
                        strip.modulators.push(Modulator {
                            id,
                            kind: ModKind::Lfo {
                                shape: ModShape::Sine,
                                rate_beats: 4.0,
                                free: false,
                                hz: 1.0,
                            },
                        });
                    }
                    if let Some(track) = strip.track
                        && ui
                            .small_button("+ follow")
                            .on_hover_text("a follower listening to the selected track's level")
                            .clicked()
                    {
                        let id = *strip.next_id;
                        *strip.next_id += 1;
                        strip.modulators.push(Modulator {
                            id,
                            kind: ModKind::Follower { track },
                        });
                    }
                });
                let mut remove: Option<u64> = None;
                let mut add_wire: Option<(u64, String)> = None;
                ui.horizontal_wrapped(|ui| {
                    let mut lfo_no = 0;
                    for modulator in strip.modulators.iter_mut() {
                        let (rect, _) = ui
                            .allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::hover());
                        // Owned, not borrowed: the chip helper needs `&mut ui`
                        // while this painter is still in scope.
                        let painter = ui.painter().clone();
                        painter.rect_filled(rect, 3.0, theme.surface_sunken);
                        painter.rect_stroke(
                            rect,
                            3.0,
                            egui::Stroke::new(1.0, theme.divider),
                            egui::StrokeKind::Inside,
                        );
                        let value = strip.values.get(&modulator.id).copied().unwrap_or(0.0);
                        // The card's destinations, written on it: the first target
                        // by name, the rest as a count. A source that drives
                        // nothing says so.
                        let mut targets = strip
                            .wires
                            .iter()
                            .filter(|wire| wire.source == modulator.id)
                            .map(|wire| {
                                strip
                                    .registry
                                    .spec(&wire.target)
                                    .map_or(wire.target.clone(), |spec| spec.name.clone())
                            });
                        let first_target = targets.next();
                        let extra = targets.count();
                        let destination = match (first_target, extra) {
                            (None, _) => "→ unwired".to_owned(),
                            (Some(name), 0) => format!("→ {name}"),
                            (Some(name), extra) => format!("→ {name} +{extra}"),
                        };
                        let wired = destination != "→ unwired";

                        // Header: name left, wire and delete right.
                        let name = match &modulator.kind {
                            ModKind::Lfo { .. } => {
                                lfo_no += 1;
                                format!("LFO {lfo_no}")
                            }
                            ModKind::Follower { track } => format!(
                                "FLW {}",
                                strip
                                    .track_names
                                    .get(*track)
                                    .map_or("?", |name| name.as_str())
                            ),
                        };
                        painter.with_clip_rect(rect).text(
                            rect.left_top() + egui::vec2(5.0, 4.0),
                            egui::Align2::LEFT_TOP,
                            &name,
                            egui::FontId::proportional(10.0),
                            theme.text,
                        );
                        painter.with_clip_rect(rect).text(
                            rect.left_top() + egui::vec2(5.0, 16.0),
                            egui::Align2::LEFT_TOP,
                            &destination,
                            font.clone(),
                            if wired {
                                theme.accent
                            } else {
                                theme.text_muted
                            },
                        );

                        match &mut modulator.kind {
                            ModKind::Lfo {
                                shape,
                                rate_beats,
                                free,
                                hz,
                            } => {
                                // The waveform, with the one moving dot.
                                let wave_rect = egui::Rect::from_min_max(
                                    egui::pos2(rect.left() + 6.0, rect.top() + 30.0),
                                    egui::pos2(rect.right() - 6.0, rect.bottom() - 22.0),
                                );
                                let points: Vec<_> = (0..=32)
                                    .map(|step| {
                                        let phase = step as f32 / 32.0;
                                        egui::pos2(
                                            wave_rect.left() + phase * wave_rect.width(),
                                            wave_rect.center().y
                                                - shape.wave(phase) * wave_rect.height() * 0.5,
                                        )
                                    })
                                    .collect();
                                painter.add(egui::Shape::line(
                                    points,
                                    egui::Stroke::new(1.0, theme.accent_muted),
                                ));
                                let phase = if *free {
                                    (strip.seconds * hz.max(1e-3)).rem_euclid(1.0)
                                } else {
                                    (strip.beat / rate_beats.max(1e-3)).rem_euclid(1.0)
                                };
                                painter.circle_filled(
                                    egui::pos2(
                                        wave_rect.left() + phase * wave_rect.width(),
                                        wave_rect.center().y - value * wave_rect.height() * 0.5,
                                    ),
                                    2.5,
                                    theme.accent,
                                );

                                // The chip row: shape, rate, and the MODE — sync
                                // rides the beat, free rides the clock.
                                let chip = |n: f32| {
                                    egui::Rect::from_min_size(
                                        egui::pos2(
                                            rect.left() + 5.0 + n * 46.0,
                                            rect.bottom() - 18.0,
                                        ),
                                        egui::vec2(42.0, 14.0),
                                    )
                                };
                                let draw_chip =
                                    |ui: &mut egui::Ui,
                                     rect: egui::Rect,
                                     id: egui::Id,
                                     text: String,
                                     on: bool| {
                                        let response = ui
                                            .interact(rect, id, egui::Sense::click())
                                            .affords(Affords::Press);
                                        let painter = ui.painter();
                                        painter.rect_filled(
                                            rect,
                                            2.0,
                                            if response.hovered() {
                                                theme.surface_raised
                                            } else {
                                                theme.surface
                                            },
                                        );
                                        painter.text(
                                            rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            text,
                                            egui::FontId::proportional(9.0),
                                            if on { theme.text } else { theme.text_muted },
                                        );
                                        response.clicked()
                                    };
                                if draw_chip(
                                    ui,
                                    chip(0.0),
                                    ui.id().with(("mod_shape", modulator.id)),
                                    shape.label().to_owned(),
                                    true,
                                ) {
                                    *shape = shape.next();
                                }
                                let rate_text = if *free {
                                    format!("{hz}Hz")
                                } else {
                                    rate_label(*rate_beats)
                                };
                                if draw_chip(
                                    ui,
                                    chip(1.0),
                                    ui.id().with(("mod_rate", modulator.id)),
                                    rate_text,
                                    true,
                                ) {
                                    if *free {
                                        let at = MOD_HZ
                                            .iter()
                                            .position(|rate| (rate - *hz).abs() < 1e-3)
                                            .unwrap_or(0);
                                        *hz = MOD_HZ[(at + 1) % MOD_HZ.len()];
                                    } else {
                                        let at = MOD_RATES
                                            .iter()
                                            .position(|rate| (rate - *rate_beats).abs() < 1e-3)
                                            .unwrap_or(0);
                                        *rate_beats = MOD_RATES[(at + 1) % MOD_RATES.len()];
                                    }
                                }
                                if draw_chip(
                                    ui,
                                    chip(2.0),
                                    ui.id().with(("mod_mode", modulator.id)),
                                    if *free { "free" } else { "sync" }.to_owned(),
                                    *free,
                                ) {
                                    *free = !*free;
                                }
                            }
                            ModKind::Follower { .. } => {
                                // One thin live bar, tall through the card's body.
                                let bar = egui::Rect::from_min_max(
                                    egui::pos2(rect.right() - 12.0, rect.top() + 30.0),
                                    egui::pos2(rect.right() - 7.0, rect.bottom() - 8.0),
                                );
                                painter.rect_filled(bar, 1.0, theme.surface);
                                let level = value.clamp(0.0, 1.0);
                                painter.rect_filled(
                                    egui::Rect::from_min_max(
                                        egui::pos2(bar.left(), bar.bottom() - bar.height() * level),
                                        bar.max,
                                    ),
                                    1.0,
                                    theme.meter_low,
                                );
                                painter.with_clip_rect(rect).text(
                                    egui::pos2(rect.left() + 5.0, rect.bottom() - 11.0),
                                    egui::Align2::LEFT_CENTER,
                                    "level of the named track",
                                    font.clone(),
                                    theme.text_muted,
                                );
                            }
                        }

                        // The wire verb: a popup of what the SELECTED track can
                        // take, one click per wire.
                        let wire_rect = egui::Rect::from_min_size(
                            egui::pos2(rect.right() - 30.0, rect.top() + 3.0),
                            egui::vec2(13.0, 13.0),
                        );
                        let wire_id = ui.id().with(("mod_wire", modulator.id));
                        let wire_response = ui
                            .interact(wire_rect, wire_id, egui::Sense::click())
                            .affords(Affords::Press);
                        painter.text(
                            wire_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "→",
                            egui::FontId::proportional(11.0),
                            if wire_response.hovered() {
                                theme.accent
                            } else {
                                theme.text_muted
                            },
                        );
                        egui::Popup::menu(&wire_response)
                            .id(wire_id.with("popup"))
                            .show(|ui| {
                                for entry in &strip.targets {
                                    if ui
                                        .button(format!("{} {}", entry.group, entry.name))
                                        .clicked()
                                    {
                                        add_wire = Some((modulator.id, entry.id.clone()));
                                        egui::Popup::close_all(ui.ctx());
                                    }
                                }
                            });
                        // Delete, quietly in the corner.
                        let x_rect = egui::Rect::from_min_size(
                            egui::pos2(rect.right() - 16.0, rect.top() + 3.0),
                            egui::vec2(13.0, 13.0),
                        );
                        let x_id = ui.id().with(("mod_x", modulator.id));
                        let x_response = ui
                            .interact(x_rect, x_id, egui::Sense::click())
                            .affords(Affords::Press);
                        if x_response.clicked() {
                            remove = Some(modulator.id);
                        }
                        painter.text(
                            x_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "×",
                            egui::FontId::proportional(11.0),
                            if x_response.hovered() {
                                theme.danger
                            } else {
                                theme.text_muted
                            },
                        );
                    }
                });
                if let Some(id) = remove {
                    strip.modulators.retain(|modulator| modulator.id != id);
                    strip.wires.retain(|wire| wire.source != id);
                }
                if let Some((source, target)) = add_wire
                    && let Some(track) = strip.track
                {
                    let id = *strip.next_id;
                    *strip.next_id += 1;
                    strip.wires.push(ModWire {
                        id,
                        source,
                        track,
                        target,
                        depth: 0.25,
                        ..Default::default()
                    });
                }

                // --- this track's wires, as sentences -------------------------------
                let Some(track) = strip.track else { return };
                let mut remove_wire: Option<usize> = None;
                let registry = strip.registry;
                for (index, wire) in strip.wires.iter_mut().enumerate() {
                    if wire.track != track {
                        continue;
                    }
                    let spec = registry.spec(&wire.target);
                    let span = spec.map_or(0.0, |spec| spec.max - spec.min);
                    let name = spec.map_or(wire.target.clone(), |spec| {
                        format!("{} {}", spec.group, spec.name)
                    });
                    let source_name = strip
                        .modulators
                        .iter()
                        .position(|modulator| modulator.id == wire.source)
                        .map_or("?".to_owned(), |at| match strip.modulators[at].kind {
                            ModKind::Lfo { .. } => format!("LFO {}", {
                                strip.modulators[..=at]
                                    .iter()
                                    .filter(|m| matches!(m.kind, ModKind::Lfo { .. }))
                                    .count()
                            }),
                            ModKind::Follower { .. } => "FLW".to_owned(),
                        });
                    // Depth in something a musician can read: real units where the
                    // unit is real, percent of range where it is not.
                    let depth_text = match spec.map(|spec| spec.unit.as_str()) {
                        Some("ms") => format!("±{:.0}ms", (wire.depth * span).abs()),
                        _ => format!("±{:.0}%", (wire.depth * 100.0).abs()),
                    };
                    let (row, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().max(280.0), ROW_H),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter();

                    // Bypass and solo, leftmost: the A/B every relationship earns.
                    let dot = egui::Rect::from_center_size(
                        egui::pos2(row.left() + 6.0, row.center().y),
                        egui::vec2(9.0, 9.0),
                    );
                    let dot_id = ui.id().with(("wire_on", wire.id));
                    let dot_response = ui
                        .interact(dot, dot_id, egui::Sense::click())
                        .affords(Affords::Press);
                    if dot_response.clicked() {
                        wire.enabled = !wire.enabled;
                    }
                    // Nine pixels across, so the hover has to be the whole
                    // difference: at this size a dot that does not answer
                    // is a dot nobody finds twice.
                    let lit = dot_response.hovered();
                    if wire.enabled {
                        painter.circle_filled(
                            dot.center(),
                            if lit { 4.0 } else { 3.0 },
                            theme.accent,
                        );
                    } else {
                        painter.circle_stroke(
                            dot.center(),
                            3.0,
                            egui::Stroke::new(1.0, if lit { theme.text } else { theme.text_muted }),
                        );
                    }
                    let solo = egui::Rect::from_center_size(
                        egui::pos2(row.left() + 18.0, row.center().y),
                        egui::vec2(10.0, 11.0),
                    );
                    let solo_id = ui.id().with(("wire_solo", wire.id));
                    if ui
                        .interact(solo, solo_id, egui::Sense::click())
                        .affords(Affords::Press)
                        .clicked()
                    {
                        wire.solo = !wire.solo;
                    }
                    painter.text(
                        solo.center(),
                        egui::Align2::CENTER_CENTER,
                        "S",
                        font.clone(),
                        if wire.solo {
                            theme.accent
                        } else {
                            theme.text_muted
                        },
                    );

                    // The sentence. Clicking it unfolds the wire's editor.
                    let label = egui::Rect::from_min_max(
                        egui::pos2(row.left() + 26.0, row.top()),
                        egui::pos2(row.left() + 150.0, row.bottom()),
                    );
                    let label_id = ui.id().with(("wire_label", wire.id));
                    let label_response = ui
                        .interact(label, label_id, egui::Sense::click())
                        .affords(Affords::Press);
                    if label_response.clicked() {
                        *strip.expanded = if *strip.expanded == Some(wire.id) {
                            None
                        } else {
                            Some(wire.id)
                        };
                    }
                    let expanded = *strip.expanded == Some(wire.id);
                    painter.with_clip_rect(label).text(
                        egui::pos2(label.left(), row.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("{source_name} → {name}  {depth_text}"),
                        font.clone(),
                        match (wire.enabled, label_response.hovered() || expanded) {
                            (false, _) => theme.text_muted,
                            (true, true) => theme.accent,
                            (true, false) => theme.text,
                        },
                    );

                    // The centre-zero depth slider, with a faint tick riding it to
                    // show the live contribution right now.
                    let slider = egui::Rect::from_min_max(
                        egui::pos2(row.left() + 152.0, row.top() + 5.0),
                        egui::pos2(row.right() - 18.0, row.bottom() - 5.0),
                    );
                    let slider_id = ui.id().with(("mod_depth", wire.id));
                    let response = ui
                        .interact(slider, slider_id, egui::Sense::click_and_drag())
                        .affords(Affords::Sweep);
                    if response.double_clicked() {
                        wire.depth = 0.0;
                    } else if response.dragged()
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        wire.depth = (((pos.x - slider.left()) / slider.width()) * 2.0 - 1.0)
                            .clamp(-1.0, 1.0);
                    }
                    painter.rect_filled(slider, 2.0, theme.surface_sunken);
                    let centre = slider.center().x;
                    let depth_x = centre + wire.depth * slider.width() * 0.5;
                    painter.rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(centre.min(depth_x), slider.top() + 2.0),
                            egui::pos2(centre.max(depth_x), slider.bottom() - 2.0),
                        ),
                        1.0,
                        theme.accent_muted,
                    );
                    painter.line_segment(
                        [
                            egui::pos2(centre, slider.top()),
                            egui::pos2(centre, slider.bottom()),
                        ],
                        egui::Stroke::new(1.0, theme.divider),
                    );
                    let live = if span > 0.0 {
                        strip.outputs.get(&wire.id).copied().unwrap_or(0.0) / span
                    } else {
                        0.0
                    };
                    let live_x = centre + live.clamp(-1.0, 1.0) * slider.width() * 0.5;
                    painter.line_segment(
                        [
                            egui::pos2(live_x, slider.top() + 1.0),
                            egui::pos2(live_x, slider.bottom() - 1.0),
                        ],
                        egui::Stroke::new(1.5, theme.accent),
                    );
                    let x_rect = egui::Rect::from_min_size(
                        egui::pos2(row.right() - 14.0, row.top() + 4.0),
                        egui::vec2(12.0, 12.0),
                    );
                    let x_id = ui.id().with(("mod_wire_x", wire.id));
                    let x_response = ui
                        .interact(x_rect, x_id, egui::Sense::click())
                        .affords(Affords::Press);
                    if x_response.clicked() {
                        remove_wire = Some(index);
                    }
                    painter.text(
                        x_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "×",
                        font.clone(),
                        if x_response.hovered() {
                            theme.danger
                        } else {
                            theme.text_muted
                        },
                    );

                    // --- the unfolded editor: the scope, and the chain's three ---
                    if expanded {
                        let (panel, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width().max(280.0), 38.0),
                            egui::Sense::hover(),
                        );
                        let painter = ui.painter();
                        // The scope: the wire's last few seconds, drawn from the
                        // ring buffer the pump keeps. Centre line is zero
                        // contribution; full height is the target's whole range.
                        let scope = egui::Rect::from_min_max(
                            egui::pos2(panel.left() + 4.0, panel.top() + 2.0),
                            egui::pos2(panel.left() + 152.0, panel.bottom() - 2.0),
                        );
                        painter.rect_filled(scope, 2.0, theme.surface_sunken);
                        painter.line_segment(
                            [
                                egui::pos2(scope.left(), scope.center().y),
                                egui::pos2(scope.right(), scope.center().y),
                            ],
                            egui::Stroke::new(1.0, theme.divider),
                        );
                        if let Some(history) = strip.scopes.get(&wire.id)
                            && history.len() > 1
                        {
                            let points: Vec<_> = history
                                .iter()
                                .enumerate()
                                .map(|(at, value)| {
                                    egui::pos2(
                                        scope.left()
                                            + at as f32 / (history.len() - 1) as f32
                                                * scope.width(),
                                        scope.center().y
                                            - value.clamp(-1.0, 1.0) * scope.height() * 0.5,
                                    )
                                })
                                .collect();
                            painter.add(egui::Shape::line(
                                points,
                                egui::Stroke::new(1.0, theme.accent),
                            ));
                        }
                        // crv / stp / lag — the chain, three micro controls.
                        let micro = |n: usize| {
                            egui::Rect::from_min_size(
                                egui::pos2(
                                    scope.right() + 6.0 + n as f32 * 54.0,
                                    panel.top() + 10.0,
                                ),
                                egui::vec2(50.0, 16.0),
                            )
                        };
                        let crv = micro(0);
                        let crv_id = ui.id().with(("wire_crv", wire.id));
                        let crv_response = ui
                            .interact(crv, crv_id, egui::Sense::click_and_drag())
                            .affords(Affords::Sweep);
                        if crv_response.double_clicked() {
                            wire.curve = 0.0;
                        } else if crv_response.dragged() {
                            wire.curve =
                                (wire.curve + crv_response.drag_delta().x / 80.0).clamp(-1.0, 1.0);
                        }
                        painter.rect_filled(crv, 2.0, theme.surface_sunken);
                        painter.text(
                            crv.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("crv {:+.1}", wire.curve),
                            font.clone(),
                            if wire.curve != 0.0 {
                                theme.text
                            } else {
                                theme.text_muted
                            },
                        );
                        let stp = micro(1);
                        let stp_id = ui.id().with(("wire_stp", wire.id));
                        if ui
                            .interact(stp, stp_id, egui::Sense::click())
                            .affords(Affords::Press)
                            .clicked()
                        {
                            const LADDER: [u32; 6] = [0, 2, 3, 4, 8, 16];
                            let at = LADDER
                                .iter()
                                .position(|steps| *steps == wire.steps)
                                .unwrap_or(0);
                            wire.steps = LADDER[(at + 1) % LADDER.len()];
                        }
                        painter.rect_filled(stp, 2.0, theme.surface_sunken);
                        painter.text(
                            stp.center(),
                            egui::Align2::CENTER_CENTER,
                            if wire.steps > 1 {
                                format!("stp {}", wire.steps)
                            } else {
                                "stp —".to_owned()
                            },
                            font.clone(),
                            if wire.steps > 1 {
                                theme.text
                            } else {
                                theme.text_muted
                            },
                        );
                        let lag = micro(2);
                        let lag_id = ui.id().with(("wire_lag", wire.id));
                        let lag_response = ui
                            .interact(lag, lag_id, egui::Sense::click_and_drag())
                            .affords(Affords::Sweep);
                        if lag_response.double_clicked() {
                            wire.smooth_ms = 0.0;
                        } else if lag_response.dragged() {
                            wire.smooth_ms = (wire.smooth_ms + lag_response.drag_delta().x * 4.0)
                                .clamp(0.0, 2_000.0);
                        }
                        painter.rect_filled(lag, 2.0, theme.surface_sunken);
                        painter.text(
                            lag.center(),
                            egui::Align2::CENTER_CENTER,
                            if wire.smooth_ms > 0.0 {
                                format!("lag {:.0}ms", wire.smooth_ms)
                            } else {
                                "lag —".to_owned()
                            },
                            font.clone(),
                            if wire.smooth_ms > 0.0 {
                                theme.text
                            } else {
                                theme.text_muted
                            },
                        );
                    }
                }
                if let Some(index) = remove_wire {
                    strip.wires.remove(index);
                }
            });
        });
}

/// A rate as musicians say it: beats up to a bar, bars past it.
pub(crate) fn rate_label(rate_beats: f32) -> String {
    if rate_beats < 4.0 {
        format!("{rate_beats}b")
    } else {
        format!("{}bar", rate_beats / 4.0)
    }
}

/// What the device region produced this frame, by the INSTANCE that
/// produced it. Two cards can share a parameter numbering without sharing a
/// node, and an edit that lost its instance would send a reverb's mix to a
/// synth's gain.
#[derive(Default)]
pub(crate) struct DeviceEdits {
    pub(crate) edits: Vec<(u64, Vec<device::ParamEdit>)>,
    /// A rack whose name or macro assignments changed: `(instance, state)`.
    ///
    /// NOT an edit: a macro assignment is not a parameter and has no wire
    /// id — it is the half of a rack that its (empty) parameter table
    /// cannot supply, and it lands in `Track::racks` beside the chain.
    pub(crate) racks: Vec<(u64, device::RackUi)>,
    /// A macro was turned: the parameter it drives, already in engine
    /// units. `(target instance, edit)`.
    ///
    /// Resolved HERE rather than by the caller because the rack knows
    /// which device the target is — it has the chain in front of it — and
    /// the value cannot be computed without knowing the device's kind.
    pub(crate) macro_moves: Vec<(u64, device::ParamEdit)>,
    /// A card's title was pressed: `(instance, additive)`.
    ///
    /// Additive means Ctrl or Shift was held, which is what makes a
    /// selection of more than one device possible — and a selection of
    /// more than one is the whole point, because grouping two devices out
    /// of five is the ordinary case.
    pub(crate) select: Option<(u64, bool)>,
    /// A card was carried onto another: `(moved, landed before)`.
    pub(crate) reorder: Option<(u64, u64)>,
    /// Where a card's display is looking now: `(instance, zoom, scroll)`.
    pub(crate) views: Vec<(u64, f32, f32)>,
    /// A card asked for its display FULL SIZE.
    pub(crate) expand: Option<u64>,
    /// A sampler's slice marker was dragged: `(instance, index, frame)`.
    ///
    /// NOT an edit: a slice table is compiled data, not a parameter, and
    /// routing it through the letter path would mean inventing a wire id
    /// for every marker.
    pub(crate) slice_moves: Vec<(u64, usize, u64)>,
    /// Samplers whose slice table should be rebuilt from their current
    /// `slices` / `slicefrom` settings.
    pub(crate) reslice: Vec<u64>,
    /// The sampler the pointer is over while a browser drag is live.
    ///
    /// REPORTED, not consumed. The release handler runs EARLIER in the
    /// frame than the rack draws, so a card that tried to take the
    /// payload itself would always be too late — by the time it drew, the
    /// drag was already over and the payload gone. This is the same shape
    /// `DragImport::spot` uses for the timeline: note where the pointer
    /// is, and let the next frame's release read it.
    pub(crate) hover_sampler: Option<u64>,
    /// Which page of a tabbed card the user turned to, by instance.
    ///
    /// Not a parameter — nothing engine-facing changes — but it is part of
    /// the patch, so it rides back the same way an edit does rather than
    /// through a side channel the project file cannot see.
    pub(crate) pages: Vec<(u64, u8)>,
}

/// The collapsed MOD strip's slim tab, and the expanded strip's width.
pub(crate) const MOD_TAB_W: f32 = 22.0;
pub(crate) const MOD_STRIP_W: f32 = 336.0;

/// How many columns a sampler's waveform picture is reduced to.
///
/// Four thousand and ninety-six is about eight times what the plot has
/// pixels, which is what keeps the waveform sharp rather than blocky when
/// the display is zoomed in — and the picture never has to be rebuilt as
/// the zoom moves, which matters because rebuilding it is a scan of the
/// whole file. Past 8x it does go blocky; that is the trade, and 48 KB a
/// sampler is what it costs.
pub(crate) const SAMPLER_WAVE_COLUMNS: usize = 4_096;

/// A sampler's file, reduced to what the card draws.
///
/// Cached on the app and rebuilt only when the PATH changes, because
/// building it is a linear scan of up to five minutes of audio and the
/// card is redrawn sixty times a second.
#[derive(Debug, Default, Clone)]
pub(crate) struct SamplerFace {
    /// What it was built from, so a stale picture is detectable.
    pub(crate) path: PathBuf,
    /// The file's own name, which is what the plot prints.
    pub(crate) name: String,
    pub(crate) wave: Vec<device::WaveColumn>,
    /// Frames at the DEVICE rate — the same units the slice table is in,
    /// so a marker drawn at `slice / frames` lands where it plays. The
    /// source file's own length would be wrong by the resampling ratio.
    pub(crate) frames: u64,
    pub(crate) truncated: bool,
    pub(crate) original_rate: u32,
}

/// Reduce loaded material to [`SAMPLER_WAVE_COLUMNS`] columns.
///
/// Min, max and RMS per column. The RMS is carried rather than derived
/// because it cannot be: two columns with the same extremes can hold
/// wildly different energy, and that difference is what makes a quiet
/// passage inside a loud file legible at card size.
pub(crate) fn sampler_wave(material: &daw::audio::material::Material) -> Vec<device::WaveColumn> {
    let frames = material.frames;
    if frames == 0 {
        return Vec::new();
    }
    (0..SAMPLER_WAVE_COLUMNS)
        .map(|column| {
            let from = (frames * column as u64 / SAMPLER_WAVE_COLUMNS as u64) as usize;
            let to = ((frames * (column + 1) as u64 / SAMPLER_WAVE_COLUMNS as u64) as usize)
                .max(from + 1)
                .min(frames as usize);
            let mut out = device::WaveColumn::default();
            let mut energy = 0.0f64;
            let mut counted = 0usize;
            for channel in 0..material.channels {
                let Some(span) = material.channel(channel).get(from..to) else {
                    continue;
                };
                for s in span {
                    out.min = out.min.min(*s);
                    out.max = out.max.max(*s);
                    energy += f64::from(*s) * f64::from(*s);
                }
                counted += span.len();
            }
            if counted > 0 {
                out.rms = (energy / counted as f64).sqrt() as f32;
            }
            out
        })
        .collect()
}

/// What a macro should print for the parameter it drives.
///
/// The device's own label table, which is the same list the modulation
/// matrix reads — so a macro and a mod wire pointed at one parameter
/// call it the same thing.
pub(crate) fn param_label(kind: DeviceKind, param: u32) -> String {
    let spec = kind.spec();
    spec.params
        .iter()
        .position(|def| def.id == param)
        .and_then(|at| spec.labels.get(at))
        .map(|label| label.name.to_lowercase())
        .unwrap_or_else(|| format!("param {param}"))
}

/// Draw ONE device's card, and everything that happens around it.
///
/// Lifted out of `device_body`'s chain loop UNCHANGED, so that a rack can
/// call it for its children while the loop calls it for everything else.
/// A rack draws cards inside a card, and the alternative to one function
/// was the same two hundred lines written twice.
///
/// Returns the edits the card made, which the caller pushes to the engine
/// — and which a rack ALSO reads, because "the parameter the user last
/// touched" is how a macro is mapped.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_device_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    instance: &DeviceInstance,
    sample_rate: f32,
    histories: &HashMap<u64, device::scope::History>,
    samplers: &HashMap<u64, SamplerFace>,
    slices: &HashMap<u64, Vec<u64>>,
    selected: &std::collections::BTreeSet<u64>,
    // Where this card sits in the chain. The drop line needs it: a card
    // lands after its target going right and before it going left, and
    // an id alone says nothing about which way that is.
    index: usize,
    edits: &mut DeviceEdits,
) -> Vec<device::ParamEdit> {
    // The card speaks normalized knob positions and
    // the instance stores engine units, so the
    // position is derived here and thrown away: the
    // stored value is the one truth, and the edit
    // coming back out is already in engine units.
    // The card's own rect, so a file dropped on
    // THIS card lands on THIS device. Taken from a
    // scope rather than from the card, because a
    // card returns what the user changed and its
    // geometry is the rack's business.
    let drawn = ui.scope(|ui| match instance.state {
        // A rack has no card of its own: it IS a card, drawn by the chain
        // loop with its children inside it. Reaching here would mean a
        // rack had been asked to draw itself as a leaf, which is a bug in
        // the caller rather than something to render.
        DeviceState::Rack => Vec::new(),
        DeviceState::Sampler(params) => {
            let mut knobs = device::SamplerUi::from_engine(|id| params.get(id).unwrap_or_default());
            let face = samplers.get(&instance.id);
            let view = device::SamplerView {
                name: face.map_or("", |f| f.name.as_str()),
                wave: face.map_or(&[][..], |f| f.wave.as_slice()),
                frames: face.map_or(0, |f| f.frames),
                slices: slices.get(&instance.id).map_or(&[][..], Vec::as_slice),
                // The engine does not report read
                // positions yet, so the plot draws
                // no playheads. The field exists
                // so adding the telemetry is a
                // wiring change and not a shape
                // change.
                voices: &[],
                truncated: face.is_some_and(|f| f.truncated),
                original_rate: face.map_or(0, |f| f.original_rate),
            };
            let out = device::sampler_card(
                ui,
                theme,
                &mut knobs,
                instance.page,
                instance.view_zoom,
                instance.view_scroll,
                &view,
            );
            if out.page != instance.page {
                edits.pages.push((instance.id, out.page));
            }
            // Where the display is looking rides
            // the instance beside `page`, for the
            // same reason: a card is rebuilt every
            // frame and forgets everything.
            if out.zoom != instance.view_zoom || out.scroll != instance.view_scroll {
                edits.views.push((instance.id, out.zoom, out.scroll));
            }
            if let Some((index, frame)) = out.slice_moved {
                edits.slice_moves.push((instance.id, index, frame));
            }
            if out.reslice {
                edits.reslice.push(instance.id);
            }
            if out.expand {
                edits.expand = Some(instance.id);
            }
            out.edits
        }
        DeviceState::SineSynth(params) => {
            let mut knobs = synth_knobs(params);
            device::sine_synth_card(ui, theme, &mut knobs)
        }
        DeviceState::Utility(params) => {
            let mut knobs = utility_knobs(params);
            device::utility_card(ui, theme, &mut knobs)
        }
        DeviceState::Limiter(params) => {
            let mut knobs = device::LimiterUi::from_engine(|id| params.get(id));
            device::limiter_card(ui, theme, &mut knobs)
        }
        DeviceState::Filter(params) => {
            let mut knobs = device::FilterUi::from_engine(|id| params.get(id));
            device::filter_card(ui, theme, &mut knobs, sample_rate)
        }
        DeviceState::Modulato(params) => {
            let mut knobs = device::modulato::ModulatoUi::from_engine(|id| params.get(id));
            device::modulato::modulato_card(ui, theme, &mut knobs)
        }
        DeviceState::Acid(params) => {
            let mut knobs = acid_knobs(params);
            device::acid_card(ui, theme, &mut knobs)
        }
        DeviceState::Kick(params) => {
            // The app is the layer that knows
            // both sides, so the conversion
            // happens here rather than inside a
            // widget that must not see the
            // engine.
            let mut knobs = device::kick::KickUi::from_engine(|id| params.get(id));
            device::kick::kick_card(ui, theme, &mut knobs)
        }
        DeviceState::Snare(params) => {
            let mut knobs = device::SnareUi::from_engine(|id| params.get(id));
            device::snare_card(ui, theme, &mut knobs)
        }
        DeviceState::Tom(params) => {
            let mut knobs = device::TomUi::from_engine(|id| params.get(id));
            device::tom_card(ui, theme, &mut knobs)
        }
        DeviceState::Hat(params) => {
            let mut knobs = device::HatUi::from_engine(|id| params.get(id));
            device::hat_card(ui, theme, &mut knobs)
        }
        DeviceState::Handclap(params) => {
            let mut knobs = device::HandclapUi::from_engine(|id| params.get(id));
            device::handclap_card(ui, theme, &mut knobs)
        }
        DeviceState::Haze(params) => {
            let mut knobs =
                device::haze::HazeUi::from_engine(|id| params.get(id).unwrap_or_default());
            let mut page = usize::from(instance.page);
            let made = device::haze::haze_card(ui, theme, &mut knobs, &mut page);
            // The page rail is part of the card, so the page it comes
            // back on is what the instance should remember.
            let page = page.min(u8::MAX as usize) as u8;
            if page != instance.page {
                edits.pages.push((instance.id, page));
            }
            made
        }
        DeviceState::Poly(params) => {
            let mut knobs = poly_knobs(params, instance.page);
            let made = device::poly_card(ui, theme, &mut knobs);
            // The tab dots are part of the card, so
            // the page it came back on is what the
            // instance should remember.
            let page = knobs.page.min(u8::MAX as usize) as u8;
            if page != instance.page {
                edits.pages.push((instance.id, page));
            }
            made
        }
        DeviceState::Loom(params) => {
            let mut knobs = loom_knobs(params, instance.page);
            let made = device::loom_card(ui, theme, &mut knobs);
            let page = knobs.packed_view();
            if page != instance.page {
                edits.pages.push((instance.id, page));
            }
            made
        }
        DeviceState::Sat(params) => {
            let mut knobs = sat_knobs(params);
            device::sat_card(ui, theme, &mut knobs)
        }
        DeviceState::Lofi(params) => {
            let mut knobs = lofi_knobs(params);
            device::lofi_card(ui, theme, &mut knobs)
        }
        DeviceState::Sheen(params) => {
            let mut knobs = sheen_knobs(params);
            device::sheen_card(ui, theme, &mut knobs)
        }
        DeviceState::Disperser(params) => {
            let mut knobs = disperser_knobs(params);
            device::disperser_card(ui, theme, &mut knobs)
        }
        DeviceState::Tilt(params) => {
            let mut knobs = tilt_knobs(params);
            device::tilt_card(ui, theme, &mut knobs)
        }
        DeviceState::Phaser(params) => {
            let mut knobs = phaser_knobs(params);
            device::phaser_card(ui, theme, &mut knobs)
        }
        DeviceState::Echo(params) => {
            let mut knobs = echo_knobs(params);
            device::echo_card(ui, theme, &mut knobs)
        }
        DeviceState::Reverb(params) => {
            let mut knobs = reverb_knobs(params);
            device::reverb_card(ui, theme, &mut knobs)
        }
        DeviceState::Gate(params) => {
            let mut knobs = gate_knobs(params);
            device::gate_card(ui, theme, &mut knobs)
        }
        DeviceState::Strip(params) => {
            let mut knobs = strip_knobs(params);
            device::strip_card(ui, theme, &mut knobs)
        }
        DeviceState::Resyn(params) => {
            let mut knobs = resyn_knobs(params, instance.page);
            let made = device::resyn_card(ui, theme, &mut knobs);
            // The picked band is UI state the card
            // forgets every frame, so the instance
            // remembers it — the eq's road, and the
            // device-UI contract's rule 3.
            let band = knobs.selected.min(u8::MAX as usize) as u8;
            if band != instance.page {
                edits.pages.push((instance.id, band));
            }
            made
        }
        DeviceState::Glue(params) => {
            let mut knobs = glue_knobs(params);
            let history = histories.get(&instance.id).cloned().unwrap_or_default();
            device::glue_card(ui, theme, &mut knobs, &history)
        }
        DeviceState::Brick(params) => {
            let mut knobs = brick_knobs(params);
            let mut page = usize::from(instance.page);
            let voices = histories
                .get(&instance.id)
                .map(|h| h.latest().bands[0])
                .unwrap_or(0.0);
            let made = device::brick::brick_card(ui, theme, &mut knobs, &mut page, voices);
            let page = page.min(u8::MAX as usize) as u8;
            if page != instance.page {
                edits.pages.push((instance.id, page));
            }
            made
        }
        DeviceState::Quad(params) => {
            let mut knobs = quad_knobs(params);
            let mut page = usize::from(instance.page);
            let voices = histories
                .get(&instance.id)
                .map(|h| h.latest().bands[0])
                .unwrap_or(0.0);
            let made = device::quad::quad_card(ui, theme, &mut knobs, &mut page, voices);
            let page = page.min(u8::MAX as usize) as u8;
            if page != instance.page {
                edits.pages.push((instance.id, page));
            }
            made
        }
        DeviceState::Stab(params) => {
            let mut knobs = stab_knobs(params);
            let mut page = usize::from(instance.page);
            let voices = histories
                .get(&instance.id)
                .map(|h| h.latest().bands[0])
                .unwrap_or(0.0);
            let made = device::stab::stab_card(ui, theme, &mut knobs, &mut page, voices);
            let page = page.min(u8::MAX as usize) as u8;
            if page != instance.page {
                edits.pages.push((instance.id, page));
            }
            made
        }
        DeviceState::Scomp(params) => {
            let mut knobs = scomp_knobs(params);
            let mut page = usize::from(instance.page);
            let made = device::scomp::scomp_card(ui, theme, &mut knobs, &mut page);
            let page = page.min(u8::MAX as usize) as u8;
            if page != instance.page {
                edits.pages.push((instance.id, page));
            }
            made
        }
        DeviceState::Tine(params) => {
            let mut knobs = tine_knobs(params);
            let voices = histories
                .get(&instance.id)
                .map(|h| h.latest().bands[0])
                .unwrap_or(0.0);
            device::tine::tine_card(ui, theme, &mut knobs, voices)
        }
        DeviceState::Tone(params) => {
            let mut knobs = tone_knobs(params);
            device::tone::tone_card(ui, theme, &mut knobs)
        }
        DeviceState::Sigil(params) => {
            let mut knobs = sigil_knobs(params);
            device::sigil::sigil_card(ui, theme, &mut knobs)
        }
        DeviceState::Gauge(params) => {
            let mut knobs = gauge_knobs(params);
            // Peak and RMS ride the two fields the readout already has;
            // the correlation rides the first band.
            let said = histories
                .get(&instance.id)
                .map(|h| h.latest())
                .unwrap_or_default();
            let history = histories.get(&instance.id).cloned().unwrap_or_default();
            device::gauge::gauge_card(
                ui,
                theme,
                &mut knobs,
                device::gauge::Reading {
                    peak_db: said.level_db,
                    rms_db: said.reduction_db,
                    correlation: said.bands[0],
                },
                &history,
            )
        }
        DeviceState::Umbra(params) => {
            let mut knobs = umbra_knobs(params);
            // The SMOOTHED depth, not the knob: the chain runs on it, and
            // the figure should show what is sounding.
            let depth = histories
                .get(&instance.id)
                .map(|h| h.latest().bands[0])
                .unwrap_or(0.0);
            device::umbra::umbra_card(ui, theme, &mut knobs, depth)
        }
        DeviceState::Ferric(params) => {
            let mut knobs = ferric_knobs(params);
            // Which grid step the head is on, and how far back it was
            // placed — the two numbers the transport diagram draws.
            let said = histories
                .get(&instance.id)
                .map(|h| h.latest())
                .unwrap_or_default();
            device::ferric::ferric_card(
                ui,
                theme,
                &mut knobs,
                device::ferric::Transport {
                    step: said.bands[0],
                    reach: said.bands[1],
                    record_db: said.level_db,
                    div_ms: said.bands[2],
                },
            )
        }
        DeviceState::Sibyl(params) => {
            let mut knobs = sibyl_knobs(params);
            // The note the engine is following. `bands[0]` is negative
            // when it has lost the pitch, which the card must draw as a
            // wheel with its spokes out rather than a stale answer.
            let midi = histories
                .get(&instance.id)
                .map(|h| h.latest().bands[0])
                .unwrap_or(-1.0);
            let heard = (midi >= 0.0).then_some(midi);
            device::sibyl::sibyl_card(ui, theme, &mut knobs, heard)
        }
        DeviceState::Flint(params) => {
            let mut knobs = flint_knobs(params);
            // The live strike weight rides on the plot: the picture is
            // what the detector WOULD do to a reference hit, and this is
            // what it is doing to the real one.
            let weight = histories
                .get(&instance.id)
                .map(|h| h.latest().bands[0])
                .unwrap_or(0.0);
            device::flint::flint_card(ui, theme, &mut knobs, weight)
        }
        DeviceState::Clamp(params) => {
            let mut knobs = clamp_knobs(params);
            // The transfer curve is a static map with no time axis, and
            // "is it fast" is the whole question this device answers —
            // so the newest reading rides on top of it: what the engine
            // is doing right now, against what it said it would do.
            let said = histories
                .get(&instance.id)
                .map(|h| h.latest())
                .unwrap_or_default();
            device::clamp::clamp_card(
                ui,
                theme,
                &mut knobs,
                said.reduction_db,
                Some(said.level_db),
            )
        }
        DeviceState::Prism(params) => {
            let mut knobs = prism_knobs(params, instance.page);
            // The beams ARE the telemetry: a multiband's one useful
            // picture is which of its three bands is working, and a
            // card is rebuilt every frame, so the reading has to
            // arrive from outside.
            let said = histories
                .get(&instance.id)
                .map(|h| h.latest())
                .unwrap_or_default();
            let made = device::prism::prism_card(ui, theme, &mut knobs, said);
            // Clicking a band picks the one the cell row edits, so the
            // band it came back on is what the instance should
            // remember — the same road the equaliser's tab takes, and
            // for the same reason: the card is a temporary.
            let band = knobs.selected.min(u8::MAX as usize) as u8;
            if band != instance.page {
                edits.pages.push((instance.id, band));
            }
            made
        }
        DeviceState::Eq(params) => {
            let mut knobs = eq_knobs(params, instance.page);
            let made = device::eq_card(ui, theme, &mut knobs, sample_rate);
            // Clicking a handle picks the band the
            // cell row edits, so the band it came
            // back on is what the instance should
            // remember — the same road the poly
            // synth's tab takes, and for the same
            // reason: the card itself is a
            // temporary and forgets everything.
            let band = knobs.selected.min(u8::MAX as usize) as u8;
            if band != instance.page {
                edits.pages.push((instance.id, band));
            }
            made
        }
    });
    let made = drawn.inner;
    // The card's handle, claimed AFTER its face so every knob on it wins
    // the pointer where the two overlap — the strip is what is left over,
    // which is exactly what a handle should be.
    let grip = device::card::grip(
        ui,
        theme,
        device::card::Handle {
            id: ui.id().with(("device_grip", instance.id)),
            card: drawn.response.rect,
            // The title band is the top of the card down to the rule the
            // card paints under its name. Asked of the card rather than
            // rebuilt from the font, so the two cannot drift.
            title: device::card::title_band(theme, drawn.response.rect),
            instance: instance.id,
            index,
            name: instance.kind().spec().name,
            selected: selected.contains(&instance.id),
            // Keep off the page dots. The grip is claimed after the
            // card's face and egui gives a press to the last widget at
            // a position, so without this the strip swallows every tab
            // underneath it — which it did, and the sampler's pages
            // could not be clicked at all.
            keep_clear: device::card::tabs_width(ui, theme, card_pages(instance.kind())),
        },
    );
    if grip.clicked {
        edits.select = Some((instance.id, grip.additive));
    }
    if let Some(from) = grip.dropped_from {
        edits.reorder = Some((from, instance.id));
    }
    // Is a browser drag hovering THIS sampler? Only
    // NOTED, never taken — see `hover_sampler`.
    if instance.kind() == DeviceKind::Sampler
        && egui::DragAndDrop::has_payload_of_type::<SampleDrag>(ui.ctx())
        && ui
            .ctx()
            .pointer_latest_pos()
            .is_some_and(|at| drawn.response.rect.contains(at))
    {
        edits.hover_sampler = Some(instance.id);
        // And say so. A drop target that looks
        // exactly like everything else is a drop
        // target nobody finds.
        ui.painter().rect_stroke(
            drawn.response.rect,
            0.0,
            egui::Stroke::new(stroke::BOLD, theme.accent),
            egui::StrokeKind::Inside,
        );
    }
    if !made.is_empty() {
        edits.edits.push((instance.id, made.clone()));
    }
    made
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn device_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    chain: &[DeviceInstance],
    strip: ModStrip<'_>,
    mod_collapsed: &mut bool,
    sample_rate: f32,
    histories: &HashMap<u64, device::scope::History>,
    // `samplers` is every sampler's loaded file by instance id — the
    // half of a sampler's card that its own parameters cannot supply.
    // `slices` is its slice table, separate because that changes without
    // the file changing.
    samplers: &HashMap<u64, SamplerFace>,
    slices: &HashMap<u64, Vec<u64>>,
    // Each rack's name and macros, by instance id — the half of a rack
    // its empty parameter table cannot supply.
    racks: &std::collections::BTreeMap<u64, device::RackUi>,
    // Which devices are picked, by instance id. View state the app owns:
    // a chain is rebuilt from the track every frame and could not hold a
    // selection between them.
    selected: &std::collections::BTreeSet<u64>,
) -> DeviceEdits {
    let mut edits = DeviceEdits::default();
    // The MOD strip is PINNED at the panel's right edge, outside the
    // rack's scroll: modulation must not live at the end of a hallway.
    // Collapsed it folds to a slim tab, so a rack that needs the room can
    // have it without the strip vanishing from the map.
    let area = ui.max_rect();
    let strip_w = if *mod_collapsed {
        MOD_TAB_W
    } else {
        MOD_STRIP_W
    };
    let strip_rect =
        egui::Rect::from_min_max(egui::pos2(area.right() - strip_w, area.top()), area.max);
    let rack_rect =
        egui::Rect::from_min_max(area.min, egui::pos2(strip_rect.left(), area.bottom()));
    ui.painter().line_segment(
        [strip_rect.left_top(), strip_rect.left_bottom()],
        egui::Stroke::new(stroke::HAIR, theme.divider),
    );

    if *mod_collapsed {
        // The tab: three stacked letters, a live dot when any modulator
        // exists — collapsed must not mean forgotten — and one click to
        // reopen.
        let id = ui.id().with("mod_tab");
        let response = ui
            .interact(strip_rect, id, egui::Sense::click())
            .affords(Affords::Press);
        if response.clicked() {
            *mod_collapsed = false;
        }
        if response.hovered() {
            ui.painter()
                .rect_filled(strip_rect, 0.0, theme.surface_raised);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let letters = ["M", "O", "D"];
        for (index, letter) in letters.iter().enumerate() {
            ui.painter().text(
                egui::pos2(
                    strip_rect.center().x,
                    strip_rect.top() + 14.0 + index as f32 * 11.0,
                ),
                egui::Align2::CENTER_CENTER,
                *letter,
                egui::FontId::proportional(9.0),
                theme.text_muted,
            );
        }
        if !strip.modulators.is_empty() {
            ui.painter().circle_filled(
                egui::pos2(strip_rect.center().x, strip_rect.top() + 52.0),
                2.0,
                theme.accent,
            );
        }
    } else {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(strip_rect.shrink2(egui::vec2(6.0, 4.0)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        mod_strip(&mut child, theme, strip, mod_collapsed);
    }

    // A rack grows rightward, so the region scrolls horizontally — and a
    // touchpad's two-finger VERTICAL swipe is translated onto that axis
    // too, because there is no vertical content to spend it on and "hover
    // the rack, swipe, it moves" is what the gesture means here. Wheel
    // users get the same courtesy for free.
    let mut rack = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rack_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let ui = &mut rack;
    egui::ScrollArea::horizontal()
        .id_salt("device_rack")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(egui::Margin::same(
                    theme.sp(daw::ui::tokens::space::SM) as i8
                ))
                .show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        // The selected track's chain, left to right in
                        // signal order: instrument first, then its effects.
                        // Edits leave as (param id, natural value) data
                        // tagged with the instance that made them; the app
                        // layer turns them into engine letters.
                        if chain.is_empty() {
                            daw::ui::kit::empty_state(ui, theme, DEVICE_EMPTY);
                        }
                        // TOP LEVEL ONLY: a device inside a rack is
                        // drawn by its rack, not here. The chain stays
                        // flat and the nesting is read off `parent`,
                        // which is what made a rack affordable at all.
                        for (index, instance) in
                            chain.iter().enumerate().filter(|(_, d)| d.parent.is_none())
                        {
                            if !matches!(instance.state, DeviceState::Rack) {
                                draw_device_card(
                                    ui,
                                    theme,
                                    instance,
                                    sample_rate,
                                    histories,
                                    samplers,
                                    slices,
                                    selected,
                                    index,
                                    &mut edits,
                                );
                                continue;
                            }
                            let before = racks.get(&instance.id).cloned().unwrap_or_default();
                            let mut rack = before.clone();
                            let out = device::rack_card(ui, theme, instance.id, &mut rack, |ui| {
                                let mut touched = Vec::new();
                                for (index, child) in chain
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, d)| d.parent == Some(instance.id))
                                {
                                    let made = draw_device_card(
                                        ui,
                                        theme,
                                        child,
                                        sample_rate,
                                        histories,
                                        samplers,
                                        slices,
                                        selected,
                                        index,
                                        &mut edits,
                                    );
                                    // The same edits the engine is about
                                    // to be sent, read a second time —
                                    // which is the whole trick behind
                                    // mapping a macro to the last thing
                                    // touched.
                                    for edit in made {
                                        touched.push(device::Touched {
                                            device: child.id,
                                            param: edit.param,
                                            label: param_label(child.kind(), edit.param),
                                        });
                                    }
                                }
                                touched
                            });
                            // A macro turn becomes an ordinary parameter
                            // edit on the device it points at. No second
                            // mechanism: a macro is a remote control, not
                            // a new kind of value.
                            for moved in out.moves {
                                let Some(target) =
                                    chain.iter().find(|d| d.id == moved.target.device)
                                else {
                                    continue;
                                };
                                edits.macro_moves.push((
                                    moved.target.device,
                                    device::ParamEdit {
                                        param: moved.target.param,
                                        value: device_value(
                                            target.kind(),
                                            moved.target.param,
                                            moved.norm,
                                        ),
                                    },
                                ));
                            }
                            if rack != before {
                                edits.racks.push((instance.id, rack));
                            }
                        }
                    });
                });
            // Vertical wheel becomes horizontal rack scroll — read AFTER
            // the cards, so a wheel a hovered control already consumed
            // (device::adjust zeroes it) no longer moves the rack too.
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 && ui.rect_contains_pointer(ui.max_rect()) {
                ui.scroll_with_delta(egui::vec2(dy, 0.0));
            }
        });
    edits
}
