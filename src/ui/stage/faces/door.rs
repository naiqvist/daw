//! DOOR's face: a doorway, a ladder, an envelope and a window, with two
//! controls living in the crevices its silhouette cuts.

use super::*;

/// DOOR's face, laid out once so the key that addresses an instrument
/// and the art that answers cannot drift apart.
///
/// The LADDER carries four of them at once, because they are four
/// facts about one line: where the door decides (the threshold notch),
/// how far under that it will not change its mind (the hysteresis
/// notch), how steeply it lets go below (the slope), and how far down
/// that fall is allowed to go (the floor). The DOORWAY beside it is
/// not a control at all — it is the door, open as far as the sound has
/// opened it.
#[derive(Clone, Copy, Debug)]
struct DoorFace {
    ladder: egui::Rect,
    doorway: egui::Rect,
    /// The three parts of one envelope: the rise, the plateau, the fall.
    attack: egui::Rect,
    hold: egui::Rect,
    release: egui::Rect,
    /// The threshold notch, the hysteresis notch under it, the slope
    /// below that, and the floor it flattens onto.
    threshold: egui::Rect,
    hysteresis: egui::Rect,
    ratio: egui::Rect,
    range: egui::Rect,
    /// The key's window: two posts on a spectrum.
    key_hp: egui::Rect,
    key_lp: egui::Rect,
    /// The mode, and the beat grid the chopper runs on.
    mode: egui::Rect,
    division: egui::Rect,
    duty: egui::Rect,
}

impl DoorFace {
    fn controls(self) -> [(u32, egui::Rect); 12] {
        use crate::params::console::door as p;
        [
            (p::MODE, self.mode),
            (p::THRESHOLD, self.threshold),
            (p::RATIO, self.ratio),
            (p::ATTACK, self.attack),
            (p::HOLD, self.hold),
            (p::RELEASE, self.release),
            (p::RANGE, self.range),
            (p::KEY_HP, self.key_hp),
            (p::KEY_LP, self.key_lp),
            (p::HYSTERESIS, self.hysteresis),
            (p::DIVISION, self.division),
            (p::DUTY, self.duty),
        ]
    }

    fn control(self, param: usize) -> Option<egui::Rect> {
        self.controls()
            .into_iter()
            .find_map(|(id, rect)| (id as usize == param).then_some(rect))
    }
}

/// Where a level stands on the ladder, top being silence and bottom
/// being the quietest the face draws.
fn door_ladder_y(ladder: egui::Rect, db: f32) -> f32 {
    let at = ((db + 72.0) / 72.0).clamp(0.0, 1.0);
    egui::lerp(ladder.bottom()..=ladder.top(), at)
}

fn door_face(
    glass: egui::Rect,
    piece: egui::Rect,
    threshold_db: f32,
    hysteresis_db: f32,
    range_db: f32,
) -> DoorFace {
    let inner = glass.shrink2(egui::vec2(6.0, 4.0));
    let top_h = (inner.height() * 0.44).clamp(56.0, 104.0);
    let top = egui::Rect::from_min_max(inner.min, egui::pos2(inner.right(), inner.top() + top_h));
    let ladder_w = (top.width() * 0.34).clamp(40.0, 86.0);
    let ladder = egui::Rect::from_min_max(top.min, egui::pos2(top.left() + ladder_w, top.bottom()));
    let doorway = egui::Rect::from_min_max(
        egui::pos2(ladder.right() + 8.0, top.top()),
        egui::pos2(top.right(), top.bottom()),
    );
    // The ladder's four regions, each a band of it wide enough to take
    // the cursor's brackets.
    let notch = |db: f32| {
        let y = door_ladder_y(ladder, db);
        egui::Rect::from_min_max(
            egui::pos2(ladder.left(), y - 5.0),
            egui::pos2(ladder.right(), y + 5.0),
        )
    };
    let threshold = notch(threshold_db);
    let hysteresis = notch(threshold_db - hysteresis_db);
    let floor_db = (threshold_db - range_db).max(-72.0);
    let range = notch(floor_db);
    let ratio = egui::Rect::from_min_max(
        egui::pos2(ladder.left(), hysteresis.bottom()),
        egui::pos2(ladder.right(), range.top().max(hysteresis.bottom() + 4.0)),
    );

    let rest = egui::Rect::from_min_max(egui::pos2(inner.left(), top.bottom() + 5.0), inner.max);
    let gap = 4.0;
    let env_h = (rest.height() * 0.44).max(22.0);
    let envelope = egui::Rect::from_min_max(rest.min, egui::pos2(rest.right(), rest.top() + env_h));
    // The rise, the plateau and the fall each take a third of the
    // drawing, which is where they are grabbed.
    let third = envelope.width() / 3.0;
    let attack = egui::Rect::from_min_max(
        envelope.min,
        egui::pos2(envelope.left() + third, envelope.bottom()),
    );
    let hold = egui::Rect::from_min_max(
        egui::pos2(attack.right(), envelope.top()),
        egui::pos2(attack.right() + third, envelope.bottom()),
    );
    let release = egui::Rect::from_min_max(
        egui::pos2(hold.right(), envelope.top()),
        egui::pos2(envelope.right(), envelope.bottom()),
    );

    let key_h = ((rest.height() - env_h - gap * 2.0) * 0.42).max(12.0);
    let key = egui::Rect::from_min_max(
        egui::pos2(rest.left(), envelope.bottom() + gap),
        egui::pos2(rest.right(), envelope.bottom() + gap + key_h),
    );
    let key_hp = egui::Rect::from_min_max(key.min, egui::pos2(key.center().x, key.bottom()));
    let key_lp = egui::Rect::from_min_max(egui::pos2(key.center().x, key.top()), key.max);

    // The MODE sits in the bay cut into the right wall, and the beat
    // GRID lies along the plinth at the foot — the two crevices the
    // silhouette leaves, each holding the control that suits its shape:
    // a tall narrow slot for a switch, a long low shelf for a row of
    // cells. What is left of the glass is the foot strip between them.
    let foot = egui::Rect::from_min_max(egui::pos2(rest.left(), key.bottom() + gap), rest.max);
    let mode = bay_rect(piece, SectionKind::Door).unwrap_or_else(|| {
        egui::Rect::from_min_max(
            foot.min,
            egui::pos2(foot.left() + foot.width() * 0.2, foot.bottom()),
        )
    });
    let grid = plinth_rect(piece, SectionKind::Door).unwrap_or(foot);
    // The grid says two things: how many cells (the division) and how
    // much of each is open (the duty). The top half is grabbed for one
    // and the bottom half for the other.
    let division = egui::Rect::from_min_max(grid.min, egui::pos2(grid.right(), grid.center().y));
    let duty = egui::Rect::from_min_max(egui::pos2(grid.left(), grid.center().y), grid.max);

    DoorFace {
        ladder,
        doorway,
        attack,
        hold,
        release,
        threshold,
        hysteresis,
        ratio,
        range,
        key_hp,
        key_lp,
        mode,
        division,
        duty,
    }
}

impl Layout for DoorFace {
    fn controls(&self) -> Vec<(u32, egui::Rect)> {
        DoorFace::controls(*self).to_vec()
    }
}

/// DOOR: a doorway, a ladder, an envelope and a window.
///
/// The LADDER on the left is the decision, drawn as one line: a
/// bright notch where the door opens, a dimmer one under it where
/// it will not change its mind again, a slope below that whose
/// steepness is the ratio, and a floor where the fall stops. The
/// key's own level rides the ladder as a column, so what the door
/// is listening to and what it decided are the same picture.
///
/// The DOORWAY beside it is not a control: it is the door, its leaf
/// standing as far open as the sound has opened it.
///
/// Under them, one ENVELOPE drawn as it is heard — a rise, a
/// plateau and a fall, each the width of its own time — and one
/// WINDOW showing the band the key listens through. At the foot,
/// the mode: an ear, or a beat grid whose cells are the division
/// and whose lit share is the duty.
pub(super) fn draw(face: &Face<'_>) {
    use crate::params::console::door as p;
    let painter = face.painter;
    let piece = face.piece;
    let glass = face.glass;
    let selected = face.selected;
    let phase = face.phase;
    let alpha = face.alpha;
    let edge = alpha.edge.color;
    let ink = alpha.ink.color;
    let live = motion::pulse_ink(alpha.live.color, alpha.live_dim.color, phase);
    let value = |param: u32| face.value(param);
    let said = face.said;
    let rhythm = value(p::MODE).round() as u32 == p::MODE_RHYTHM;
    let threshold_db = value(p::THRESHOLD);
    let hysteresis_db = value(p::HYSTERESIS);
    let range_db = value(p::RANGE);
    let lay = door_face(glass, piece.rect, threshold_db, hysteresis_db, range_db);
    // How far the door stands open, smoothed so the leaf swings
    // rather than snaps between frames.
    let open = painter.ctx().animate_value_with_time(
        egui::Id::new(("stage-door-open", piece.index)),
        if said.bands[1] > 0.0 {
            said.bands[1].clamp(0.0, 1.0)
        } else {
            1.0
        },
        0.09,
    );
    let mut shapes = Vec::new();

    // ---- the ladder: threshold, hysteresis, ratio, range -------
    circuit::panel_frame_variant(&mut shapes, lay.ladder, Weight::Hair, edge, 1);
    let ladder = lay.ladder.shrink2(egui::vec2(4.0, 3.0));
    let rail_x = ladder.left() + 9.0;
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(rail_x, ladder.top()),
            egui::pos2(rail_x, ladder.bottom()),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.8),
    );
    // The key's level, as a column climbing the rail.
    let key_y = door_ladder_y(lay.ladder, said.bands[2]);
    if said.bands[2] > -71.0 {
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rail_x - 3.0, key_y),
                egui::pos2(rail_x + 3.0, ladder.bottom()),
            ),
            0.0,
            live.gamma_multiply(0.85),
        ));
    }
    // The decision: the threshold's notch bright, the hysteresis
    // notch under it dim, the slope between the two floors.
    let threshold_y = door_ladder_y(lay.ladder, threshold_db);
    let hyst_y = door_ladder_y(lay.ladder, threshold_db - hysteresis_db);
    let floor_y = door_ladder_y(lay.ladder, (threshold_db - range_db).max(-72.0));
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(rail_x - 6.0, threshold_y),
            egui::pos2(ladder.right(), threshold_y),
        ],
        Weight::Heavy,
        ink,
    );
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(rail_x - 4.0, hyst_y),
            egui::pos2(ladder.right() - 6.0, hyst_y),
        ],
        Weight::Hair,
        alpha.jeopardy_latent.color,
    );
    // The slope: how fast the gain falls under the threshold. Its
    // angle IS the ratio, and it flattens where the range stops it.
    let slope_left = ladder.right() - 4.0;
    let ratio = value(p::RATIO).max(1.0);
    let reach = ((floor_y - hyst_y) / (ratio * 3.0)).clamp(4.0, ladder.width() - 14.0);
    let corner = egui::pos2(slope_left - reach, floor_y);
    circuit::trace(
        &mut shapes,
        &[egui::pos2(slope_left, hyst_y), corner],
        Weight::Heavy,
        tool::mix_ink(ink, alpha.jeopardy_active.color, 0.4),
    );
    circuit::trace(
        &mut shapes,
        &[corner, egui::pos2(ladder.left() + 2.0, floor_y)],
        Weight::Hair,
        edge.gamma_multiply(1.2),
    );

    // ---- the doorway: the leaf, as far open as the sound has it
    circuit::panel_variant(
        &mut shapes,
        lay.doorway,
        Some(alpha.well.color),
        alpha.ground.color,
        Some((Weight::Hair, edge)),
        2,
    );
    let jamb = lay.doorway.shrink(5.0);
    shapes.push(egui::Shape::rect_filled(jamb, 0.0, alpha.ground.color));
    // The frame it swings in, and the plate it closes onto.
    circuit::trace(
        &mut shapes,
        &[
            jamb.left_bottom(),
            jamb.left_top(),
            jamb.right_top(),
            jamb.right_bottom(),
        ],
        Weight::Hair,
        edge.gamma_multiply(1.1),
    );
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(egui::pos2(jamb.left(), jamb.bottom() - 2.0), jamb.max),
        0.0,
        edge,
    ));
    // The opening the leaf swings into, and the leaf itself: shut,
    // it covers the whole jamb; open, it has swung to the side.
    // The leaf never vanishes: thrown wide it stands against the
    // wall, shut it covers the opening. A door with no leaf in it
    // is a hole.
    let leaf_w = jamb.width() * (0.22 + 0.78 * (1.0 - open).clamp(0.0, 1.0));
    if leaf_w > 0.5 {
        let leaf =
            egui::Rect::from_min_max(egui::pos2(jamb.right() - leaf_w, jamb.top()), jamb.max);
        shapes.push(egui::Shape::rect_filled(
            leaf,
            0.0,
            tool::mix_ink(alpha.surface.color, alpha.jeopardy_latent.color, 0.35),
        ));
        circuit::trace(
            &mut shapes,
            &[leaf.left_top(), leaf.left_bottom()],
            Weight::Heavy,
            ink,
        );
    }
    // The light through the opening, and the hinges it swings on.
    let gap = egui::Rect::from_min_max(jamb.min, egui::pos2(jamb.right() - leaf_w, jamb.bottom()));
    if gap.width() > 1.0 {
        // What comes through: brighter the wider it stands, and a
        // sill of light along the floor of the opening.
        shapes.push(egui::Shape::rect_filled(
            gap,
            0.0,
            live.gamma_multiply(0.05 + 0.13 * open),
        ));
        shapes.push(egui::Shape::rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(gap.left(), gap.bottom() - 3.0),
                egui::pos2(gap.right(), gap.bottom()),
            ),
            0.0,
            live.gamma_multiply(0.35 + 0.5 * open),
        ));
    }
    for at in [0.25, 0.5, 0.75] {
        circuit::pad(
            &mut shapes,
            egui::pos2(jamb.right() + 1.0, egui::lerp(jamb.y_range(), at)),
            circuit::PAD - 2.0,
            edge,
            false,
        );
    }

    // ---- the envelope: rise, plateau, fall ---------------------
    let env = egui::Rect::from_min_max(lay.attack.min, lay.release.max);
    circuit::panel_frame_variant(&mut shapes, env, Weight::Hair, edge, 3);
    let shape_of = |ms: f32, most: f32| (ms / most).clamp(0.02, 1.0).sqrt();
    let rise = shape_of(value(p::ATTACK), 100.0);
    let plateau = shape_of(value(p::HOLD), 500.0);
    let fall = shape_of(value(p::RELEASE), 2_000.0);
    let inner = env.shrink2(egui::vec2(5.0, 4.0));
    let base = inner.bottom();
    let peak = inner.top();
    // Each time takes its own third of the drawing, and within that
    // third the corner slides: a long attack leans, a short one
    // stands up.
    let a_end = egui::lerp(lay.attack.x_range(), rise.clamp(0.1, 0.95));
    let h_end = egui::lerp(lay.hold.x_range(), plateau.clamp(0.05, 0.95));
    let r_end = egui::lerp(lay.release.x_range(), fall.clamp(0.1, 0.95));
    let envelope = vec![
        egui::pos2(inner.left(), base),
        egui::pos2(a_end, peak),
        egui::pos2(h_end.max(a_end), peak),
        egui::pos2(r_end.max(h_end), base),
        egui::pos2(inner.right(), base),
    ];
    circuit::trace(&mut shapes, &envelope, Weight::Heavy, ink);
    for x in [a_end, h_end] {
        circuit::trace(
            &mut shapes,
            &[egui::pos2(x, peak), egui::pos2(x, base)],
            Weight::Hair,
            edge.gamma_multiply(0.7),
        );
    }
    // The door's own gain rides the envelope's height, so the
    // drawing moves with the sound it is describing.
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(inner.left(), egui::lerp(base..=peak, open)),
            egui::pos2(inner.right(), base),
        ),
        0.0,
        live.gamma_multiply(0.12),
    ));

    // ---- the key's window -------------------------------------
    let key = egui::Rect::from_min_max(lay.key_hp.min, lay.key_lp.max);
    circuit::trace(
        &mut shapes,
        &[
            egui::pos2(key.left(), key.center().y),
            egui::pos2(key.right(), key.center().y),
        ],
        Weight::Hair,
        edge.gamma_multiply(0.8),
    );
    let hp_x = tool::octave_x(key, value(p::KEY_HP));
    let lp_x = tool::octave_x(key, value(p::KEY_LP));
    shapes.push(egui::Shape::rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(hp_x, key.top() + 2.0),
            egui::pos2(lp_x.max(hp_x + 1.0), key.bottom() - 2.0),
        ),
        0.0,
        live.gamma_multiply(0.3),
    ));
    for (x, out) in [(hp_x, true), (lp_x, false)] {
        let post = egui::Rect::from_min_max(
            egui::pos2(x - 2.0, key.top()),
            egui::pos2(x + 2.0, key.bottom()),
        );
        shapes.push(egui::Shape::rect_filled(post, 0.0, ink));
        let _ = out;
    }

    // ---- the mode, and the grid the chopper runs on ------------
    let mode = lay.mode;
    if rhythm {
        // A beat: a filled square with its own pulse.
        shapes.push(egui::Shape::rect_filled(
            mode.shrink(3.0),
            0.0,
            tool::mix_ink(alpha.live_dim.color, alpha.live.color, phase.dash()),
        ));
    } else {
        // An ear: two arcs listening.
        for r in [3.0f32, 6.0] {
            let arc: Vec<egui::Pos2> = (0..=12)
                .map(|i| on_arc(mode.center(), r, -60.0 + 120.0 * i as f32 / 12.0))
                .collect();
            circuit::trace(&mut shapes, &arc, Weight::Hair, ink);
        }
    }
    let grid = egui::Rect::from_min_max(lay.division.min, lay.duty.max);
    let cells = match value(p::DIVISION).round().max(0.0) as usize {
        0 => 2usize,
        1 => 4,
        2 => 8,
        3 => 16,
        4 => 3,
        _ => 6,
    };
    let duty = (value(p::DUTY) / 100.0).clamp(0.0, 1.0);
    let step = grid.width() / cells as f32;
    for cell in 0..cells {
        let x0 = grid.left() + step * cell as f32;
        let lit = egui::Rect::from_min_max(
            egui::pos2(x0, grid.top() + 1.0),
            egui::pos2(x0 + (step - 1.0) * duty, grid.bottom() - 1.0),
        );
        let whole = egui::Rect::from_min_max(
            egui::pos2(x0, grid.top() + 1.0),
            egui::pos2(x0 + step - 1.0, grid.bottom() - 1.0),
        );
        shapes.push(egui::Shape::rect_stroke(
            whole,
            0.0,
            egui::Stroke::new(Weight::Hair.px(), edge.gamma_multiply(0.8)),
            egui::StrokeKind::Inside,
        ));
        if lit.width() > 0.5 {
            shapes.push(egui::Shape::rect_filled(
                lit,
                0.0,
                if rhythm {
                    live.gamma_multiply(0.85)
                } else {
                    edge.gamma_multiply(1.1)
                },
            ));
        }
    }
    painter.extend(shapes);

    // The cursor: the house brackets around whichever instrument
    // the keyboard is holding.
    face.mark(&lay);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::SectionKind;

    #[test]
    fn every_door_parameter_has_one_instrument() {
        let piece = egui::Rect::from_min_size(
            egui::pos2(30.0, 40.0),
            egui::vec2(width_of(SectionKind::Door), 240.0),
        );
        let glass = recess_of(piece, SectionKind::Door).shrink(3.0);
        let face = door_face(glass, piece, -30.0, 6.0, 40.0);
        for table in SectionKind::Door.table() {
            let rect = face
                .control(table.id as usize)
                .unwrap_or_else(|| panic!("{} has no instrument", table.name));
            assert!(rect.is_positive(), "{} has no room", table.name);
            assert!(piece.contains_rect(rect), "{} left the piece", table.name);
        }
        // The ladder reads top to bottom: the threshold, the give under
        // it, the slope, then the floor.
        assert!(face.threshold.center().y < face.hysteresis.center().y);
        assert!(face.hysteresis.center().y <= face.ratio.center().y);
        assert!(face.ratio.center().y <= face.range.center().y);
        // The envelope is three parts of one drawing, in order.
        assert!(face.attack.right() <= face.hold.left() + 0.01);
        assert!(face.hold.right() <= face.release.left() + 0.01);
        // And the crevices hold what the glass has no room for.
        assert!(
            !glass.contains_rect(face.mode),
            "the mode is not in its bay"
        );
        assert!(
            !glass.contains_rect(face.division),
            "the grid is not on its plinth"
        );
    }
}
