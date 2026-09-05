//! The band: the old view's chain, transplanted whole — the console's
//! sections as interlocking pieces with their faces, cards for the
//! devices that are not sections, the crossing from one rail to the
//! next drawn as a cable, the sends as a loom. Every line here came
//! across from `mvp-port` at 2ee4b64; what changed is only what it is
//! coloured with: the design alphabet seen through the console's palette
//! (`Stage::glass`), so the faces are the faces and the room is this one.

use super::palette;
use super::*;
use crate::ui::chrome;
use crate::ui::stage::chain;
use eframe::egui;

/// A device card in the chain band. Wider than a track's column on
/// purpose: a column is an address and holds a name and a number, while
/// a card is a table of parameters — a name, a gauge and a value on
/// every row — and a table crammed to a column's width cut every name
/// to three letters. Layout dimensions, settled by eye like the rest.
const CHAIN_W: f32 = 272.0;

/// One parameter row's height in a card.
const CHAIN_PITCH: f32 = 24.0;

/// A card's head: the family seal, the terse title, and a sample's name.
const CHAIN_HEAD_H: f32 = 40.0;

/// The LOOM: a lane along the foot of the band where the two sends run
/// out to their returns and the returns run back into the mix. It costs
/// the cards one row and buys the one thing a list of devices can never
/// say — that the signal does not only go left to right.
const LOOM_H: f32 = 0.0; // no loom: the cards stand alone
#[allow(dead_code)]
const LOOM_H_WAS: f32 = 15.0;

/// The gap the band opens where it crosses from one rail of the desk to
/// the next. Wide enough for the pair to cross it visibly and for the
/// rail's name to climb beside them.
const RAIL_GAP: f32 = 34.0;

fn device_family_word(family: Family) -> &'static str {
    match family {
        Family::Synths => "SYNTHS",
        Family::Drums => "DRUMS",
        Family::Sampling => "SAMPLING",
        Family::Dynamics => "DYNAMICS",
        Family::EqAndFilters => "FILTERS",
        Family::DelayAndReverb => "TIME",
        Family::Distortion => "DRIVE",
        Family::Modulation => "MODULATION",
        Family::Spectral => "SPECTRAL",
        Family::Utilities => "UTILITY",
        Family::Console => "CONSOLE",
    }
}

/// Between one track column and the next.
///
/// WIDER than the gap between the slots stacked inside a column, and that
/// difference is the whole point: proximity is what groups marks, so the
/// eye reads a column as one thing before it reads a row. With both gaps
/// equal the lattice reads as an even mesh and neither axis wins, which
/// is what made a session of scattered clips hard to scan.
fn column_gap() -> f32 {
    design::px(design::space::STEP)
}

/// Between the slots stacked inside one column.
fn row_gap() -> f32 {
    design::px(design::space::HAIR)
}

/// Between the head and the first cell under it: NOTHING.
///
/// They touch, because they are one thing. A head and the cells beneath
/// it are a column — the head names it and the cells are what is in it —
/// and a break between them made the strip read as two stacked bands
/// that happened to line up rather than as a row of columns.
///
/// So the surface has two distances, not three: none inside a column,
/// and a gap between columns. The only boundary left is the one that
/// separates things which really are separate.
///
/// A rule was tried here first and taken out again — it drew a boundary
/// the space already drew — and then the space itself turned out to be
/// drawing a boundary that was not there.
fn section_gap() -> f32 {
    0.0
}

/// The sign for a place with nothing in it: a point, the smallest mark
/// the surface can make. An empty slot is drawn as one point rather than
/// as a plane, so the session reads as clips on a ground instead of a
/// grid of squares — the lattice is still there, in the points' rank
/// and file, but it is the ground and the clips are the figure.
const POINT: f32 = 3.0;

/// The two returns' cables. A send is followed by eye, so each return
/// owns a colour: TAPE warm like the oxide, SHADOW cold like a plate.
const RETURN_INK: [egui::Color32; 2] = [
    egui::Color32::from_rgb(230, 170, 96),
    egui::Color32::from_rgb(130, 176, 255),
];

/// A cable's ink at `amount` of its full strength.
fn tint(ink: egui::Color32, amount: f32) -> egui::Color32 {
    ink.gamma_multiply(amount.clamp(0.0, 1.0))
}

impl Stage {
    /// The alphabet the band draws in: the design alphabet, lifted
    /// through the console's palette.
    pub(super) fn glass(&self) -> design::Alphabet {
        palette::lift(*design::Alphabet::for_polarity(self.polarity))
    }

    /// The marked one. Exactly one thing on the screen is ever this.
    pub(super) fn focused(&self) -> egui::Color32 {
        self.glass().focus.color
    }

    /// Where focus WILL be when it comes back.
    pub(super) fn resting(&self) -> egui::Color32 {
        self.glass().ink.color
    }

    pub(super) fn square(&self) -> egui::Color32 {
        self.glass().surface.color
    }

    /// What a refusal is drawn in.
    pub(super) fn refusal_ink(&self) -> egui::Color32 {
        self.glass().ink.color
    }

    /// How many parameter rows the band shows at once, and only whole
    /// ones. A pure function of the tray, like every other capacity here.
    pub(super) fn chain_capacity(tray: egui::Rect) -> usize {
        let margin = design::px(design::space::ROOM);
        let cards = chain::rows_that_fit(tray.height() - margin - CHAIN_HEAD_H, CHAIN_PITCH);
        let pieces = chain::rows_that_fit(
            tray.height()
                - margin
                - (strip::HEAD_H + 4.0 + 3.0 + strip::FIGURE_MAX_H + 4.0)
                - strip::FOOT_H
                - 6.0,
            strip::ROW_H,
        );
        cards.min(pieces)
    }

    /// The tray with nothing in it: the deck's dormant face. A sigil
    /// wheel, two register columns, a spiral and the cosmological dial,
    /// all at the structure rung — present, and saying nothing, the way
    /// a shrine is carved before it lights.
    fn draw_quiet_tray(&self, painter: &egui::Painter, tray: egui::Rect) {
        let edge = self.glass().edge.color;
        let ground = self.glass().ground.color;
        kit::cached(
            painter,
            egui::Id::new("stage-quiet-tray"),
            tray,
            (edge, ground),
            |out| {
                let m = design::px(design::space::ROOM);
                let plaque = tray.shrink(m);
                if plaque.height() < 40.0 {
                    return;
                }
                // The plaque is the tray's own casing while nothing is in
                // it, so it carries the frame weight rather than a rule's.
                chrome::panel_frame_variant(out, plaque, Weight::Heavy, edge, 2);
                let c = plaque.center();
                let side = plaque.height() * 0.7;
                Sign::Dipper.paint(
                    out,
                    egui::Rect::from_center_size(c, egui::Vec2::splat(side)),
                    Weight::Heavy,
                    edge,
                );
                let mut rng = kit::Rng::seeded("quiet-tray");
                let unit = 8.0;
                for row in 0..3 {
                    let y = plaque.min.y + m + row as f32 * (unit + 2.0);
                    chrome::binary(
                        out,
                        egui::pos2(plaque.min.x + m, y),
                        unit,
                        rng.next_u64() as u32,
                        16,
                        edge,
                    );
                }
                chrome::rail(
                    out,
                    egui::pos2(plaque.max.x - m - 140.0, plaque.max.y - m),
                    egui::pos2(plaque.max.x - m, plaque.max.y - m),
                    &[0.0, 0.5, 1.0],
                    edge,
                );
            },
        );
    }

    /// The clip tray. The sequencer draws the clip in view — the stage
    /// builds what it reads (notes resolved against the key, the track's
    /// lens) — and has the keys only while the cursor is inside. While
    /// focus is elsewhere the tray is drawn and then veiled, so the
    /// sequencer's own white stays below the one focus-bright thing on
    /// the screen, and lands whatever it asked for on the pattern.
    /// The chain band: the addressed track's devices, in signal order,
    /// each carrying its whole parameter table as a scrolling list.
    pub(super) fn draw_chain(&self, painter: &egui::Painter, tray: egui::Rect, phase: Phase) {
        let Some(lattice) = self.chain.as_ref() else {
            return;
        };
        let Some(track) = self.addressed_track() else {
            return;
        };
        let columns = chain::band(&self.song, track);
        if columns.is_empty() {
            return;
        }
        let margin = design::px(design::space::ROOM);
        let gap = column_gap();
        let head_h = CHAIN_HEAD_H;
        let pitch = CHAIN_PITCH;
        let body_top = tray.min.y + head_h;
        let rows_shown = Self::chain_capacity(tray);
        let _ = chain::rows_that_fit(tray.max.y - margin - body_top, pitch);
        let cursor = lattice.cursor();

        // The band is a rail of pieces of two kinds: cards, which stand
        // apart by the column gap, and the strip's sections, which mate
        // — a section's tongue lies in the next section's notch, so two
        // sections take no gap between them.
        let widths: Vec<f32> = columns
            .iter()
            .map(|column| column.section.map_or(CHAIN_W, strip::width_of))
            .collect();
        // Two pieces mate only when they stand on the SAME rail: a
        // channel's sections are one run, its group bus's another, the
        // mix's another. Where the band crosses from one rail to the
        // next it opens a gap, and the pair crosses it as a visible
        // cable with the rail's name engraved over it — because that
        // crossing is a real thing about the desk, not a seam to hide.
        // A return mates with nothing at all: it is a parallel path.
        // Standard cards: nothing mates, nothing crosses, and the gap
        // between any two is the gap.
        let mates = |_i: usize| -> bool { false };
        // Where the band crosses rails it opens the wider gap, so the
        // cable and the rail's name have room to be seen.
        let crossing = |_i: usize| -> bool { false };
        let step = |i: usize| -> f32 {
            widths[i]
                + if mates(i) {
                    0.0
                } else if crossing(i) {
                    RAIL_GAP
                } else {
                    gap
                }
        };
        let avail = tray.width() - margin * 2.0;

        // The rail scrolls so the cursor's piece is on screen: the first
        // piece shown is the earliest from which the cursor's still fits.
        let cursor_col = cursor.map_or(0, |(col, _)| col).min(columns.len() - 1);
        let mut first = 0;
        loop {
            let mut x = 0.0;
            let mut fits = false;
            for i in first..=cursor_col {
                if x + widths[i] <= avail {
                    if i == cursor_col {
                        fits = true;
                    }
                    x += step(i);
                } else {
                    break;
                }
            }
            if fits || first >= cursor_col {
                break;
            }
            first += 1;
        }
        let mut layout: Vec<(usize, egui::Rect)> = Vec::new();
        let mut x = tray.min.x + margin;
        for i in first..columns.len() {
            if x + widths[i] > tray.max.x - margin + 0.5 {
                break;
            }
            layout.push((
                i,
                egui::Rect::from_min_max(
                    egui::pos2(x, tray.top()),
                    egui::pos2(x + widths[i], tray.bottom() - margin - LOOM_H),
                ),
            ));
            x += step(i);
        }
        if layout.is_empty() {
            return;
        }
        let sounding = self.playing_on(track).is_some();

        // The pieces: every body first, so a tongue laid afterwards lies
        // in its neighbour's notch rather than under it.
        let pieces: Vec<(strip::Piece, &chain::Column)> = layout
            .iter()
            .filter_map(|(i, rect)| {
                columns[*i].section.map(|kind| {
                    (
                        strip::Piece {
                            index: *i,
                            rect: *rect,
                            kind,
                            // A return is off the rail, so it wears a
                            // notch for its send cable and leaves by a
                            // pad rather than by a tongue.
                            notch: false,
                            tongue: mates(*i),
                        },
                        &columns[*i],
                    )
                })
            })
            .collect();
        for (piece, column) in &pieces {
            self.draw_piece_body(painter, *piece, column);
        }
        let level = self
            .meters
            .readings()
            .get(track)
            .map(|reading| reading.level.peak());
        for (piece, column) in &pieces {
            self.draw_piece_face(
                painter,
                *piece,
                column,
                cursor,
                self.chain_offset,
                rows_shown,
                sounding,
                phase,
                level,
            );
        }
        for (index, rect) in &layout {
            if columns[*index].section.is_some() {
                continue;
            }
            self.draw_chain_card(
                painter,
                *rect,
                &columns[*index],
                *index,
                cursor,
                self.chain_offset,
                rows_shown,
                head_h,
                pitch,
            );
        }
    }

    /// Where the band crosses from one rail of the desk to the next:
    /// the channel's last section into its group bus, the bus into the
    /// mix.
    ///
    /// The pair crosses a real gap, and the rail it is arriving on is
    /// named climbing beside it. This is the one place on the band
    /// where a card does NOT plug into its neighbour, and it is the
    /// place where the signal stops being one track's and becomes the
    /// desk's — so the eye is told, rather than left to guess from a
    /// change of seal.
    fn draw_rail_crossing(
        &self,
        painter: &egui::Painter,
        left: egui::Rect,
        right: egui::Rect,
        lane: chain::Lane,
        sounding: bool,
        phase: Phase,
    ) {
        let alpha = self.glass();
        let jy = strip::joint_y(right);
        let from_x = left.right();
        let to_x = right.left() + strip::TONGUE;
        let mut shapes = Vec::new();
        for dy in [-4.0, 4.0] {
            let path = [egui::pos2(from_x, jy + dy), egui::pos2(to_x, jy + dy)];
            chrome::trace(&mut shapes, &path, Weight::Hair, alpha.edge.color);
            if sounding && phase.rolling {
                chrome::dashes(
                    &mut shapes,
                    &path,
                    phase.dash(),
                    Weight::Heavy,
                    alpha.live_dim.color,
                );
            }
            chrome::pad(
                &mut shapes,
                egui::pos2(from_x, jy + dy),
                chrome::PAD - 1.0,
                alpha.edge.color,
                true,
            );
        }
        // A hairline dropped the height of the band marks the seam, so
        // the run of pieces reads as two runs rather than as one long
        // one with a gap in it.
        let seam = (from_x + to_x) * 0.5;
        chrome::trace(
            &mut shapes,
            &[
                egui::pos2(seam, right.top() + strip::HEAD_H),
                egui::pos2(seam, jy + 14.0),
            ],
            Weight::Hair,
            alpha.edge.color.gamma_multiply(0.5),
        );
        painter.extend(shapes);
    }

    /// The LOOM: the two sends leaving the channel's OUT for their
    /// returns, and the two returns landing back in the mix.
    ///
    /// A list of devices left to right can only say that the signal
    /// goes one way. It does not: OUT taps a share of the channel into
    /// TAPE and into SHADOW, and what those two make comes back into
    /// the mix beside everything else. So the sends are drawn as what
    /// they are — cables, running the length of the desk along the
    /// band's foot, out on the upper pair and home on the lower, each
    /// return its own colour. A cable is as bright as its send is open,
    /// so a closed send is a cable that is plainly not carrying
    /// anything, and the dashes on it travel with the beat.
    #[allow(clippy::too_many_arguments)]
    fn draw_loom(
        &self,
        painter: &egui::Painter,
        loom: egui::Rect,
        track: usize,
        columns: &[chain::Column],
        layout: &[(usize, egui::Rect)],
        sounding: bool,
        phase: Phase,
    ) {
        use crate::params::console::out as p;
        if !loom.is_positive() {
            return;
        }
        let alpha = self.glass();
        let seen = |at: usize| layout.iter().find(|(i, _)| *i == at).map(|(_, r)| *r);
        let column_of =
            |want: &dyn Fn(&chain::Column) -> bool| columns.iter().position(|c| want(c));

        // The three places a cable touches: where the send leaves, where
        // it lands, and where the return comes home.
        let out_col = column_of(&|column: &chain::Column| {
            column.lane == chain::Lane::Channel
                && column.section == Some(crate::console::SectionKind::Out)
        });
        let mix_col = column_of(&|column: &chain::Column| column.lane == chain::Lane::Mix);
        let out_rect = out_col.and_then(seen);
        let mix_rect = mix_col.and_then(seen);

        // How open each send is: what OUT measured of itself last block
        // when the engine is running, and what the hand set when it is
        // not.
        let sends = out_col
            .and_then(|col| chain::device_at(&self.song, track, col))
            .map(|id| {
                let said = self.telemetry(id);
                let set = self.song.device(id);
                let value = |param: u32| set.map_or(0.0, |device| device.value(param)) / 100.0;
                if said.bands[1] > 0.0 || said.bands[2] > 0.0 {
                    [said.bands[1] / 100.0, said.bands[2] / 100.0]
                } else {
                    [value(p::SEND_TAPE), value(p::SEND_SHADOW)]
                }
            })
            .unwrap_or([0.0, 0.0]);

        let mut shapes = Vec::new();
        for index in 0..2 {
            let ink = RETURN_INK[index];
            let open = sends[index].clamp(0.0, 1.0);
            let ret_col =
                column_of(&|column: &chain::Column| column.lane == chain::Lane::Return(index));
            let ret_rect = ret_col.and_then(seen);
            // Out on the upper pair, home on the lower, each return
            // keeping its own line so two cables never read as one.
            let out_y = loom.top() + 3.0 + index as f32 * 3.0;
            let home_y = loom.bottom() - 3.0 - index as f32 * 3.0;
            let carrying = open > 0.005;
            let cable = if carrying {
                tint(ink, 0.35 + 0.65 * open)
            } else {
                alpha.edge.color.gamma_multiply(0.55)
            };

            // The send: down out of OUT's foot, along the loom, up into
            // the return's notch.
            let from_x = out_rect.map_or(loom.left(), |rect| rect.center().x + 10.0 * index as f32);
            let to_x = ret_rect.map_or(loom.right(), |rect| rect.left() + strip::TONGUE);
            let send = vec![
                egui::pos2(from_x, out_rect.map_or(out_y, |rect| rect.bottom())),
                egui::pos2(from_x, out_y),
                egui::pos2(to_x, out_y),
                egui::pos2(to_x, ret_rect.map_or(out_y, |rect| strip::joint_y(rect))),
            ];
            chrome::trace(&mut shapes, &send, Weight::Hair, cable);
            if carrying && sounding && phase.rolling {
                chrome::dashes(&mut shapes, &send, phase.dash(), Weight::Heavy, ink);
            }
            if let Some(rect) = out_rect {
                // The tap: a pad on the channel's foot that fills as the
                // send opens. It is the send, not a picture of it.
                chrome::pad(
                    &mut shapes,
                    egui::pos2(from_x, rect.bottom()),
                    chrome::PAD,
                    cable,
                    carrying,
                );
            }

            // The way home: out of the return's right edge, along the
            // loom, up into the mix's notch.
            let home_from = ret_rect.map_or(loom.right(), |rect| rect.right());
            let home_to = mix_rect.map_or(loom.left(), |rect| rect.left() + strip::TONGUE);
            let home = vec![
                egui::pos2(
                    home_from,
                    ret_rect.map_or(home_y, |rect| strip::joint_y(rect)),
                ),
                egui::pos2(home_from + 6.0, home_y),
                egui::pos2(home_to, home_y),
                egui::pos2(
                    home_to,
                    mix_rect.map_or(home_y, |rect| strip::joint_y(rect)),
                ),
            ];
            chrome::trace(&mut shapes, &home, Weight::Hair, cable);
            if carrying && sounding && phase.rolling {
                chrome::dashes(&mut shapes, &home, phase.dash(), Weight::Heavy, ink);
            }
            if let Some(rect) = ret_rect {
                chrome::pad(
                    &mut shapes,
                    egui::pos2(rect.right(), strip::joint_y(rect)),
                    chrome::PAD - 1.0,
                    cable,
                    carrying,
                );
            }
        }
        painter.extend(shapes);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_chain_card(
        &self,
        painter: &egui::Painter,
        card: egui::Rect,
        column: &chain::Column,
        index: usize,
        cursor: Option<(usize, usize)>,
        row_offset: usize,
        rows_shown: usize,
        head_h: f32,
        pitch: f32,
    ) {
        let alpha = self.glass();
        let head = egui::Rect::from_min_size(card.min, egui::vec2(card.width(), head_h));
        let body_top = head.bottom();
        let row_font = egui::FontId::monospace(design::px(design::type_scale::MICRO));
        let family_ink = if column.bypassed {
            alpha.edge.color
        } else {
            alpha.ink.color
        };
        kit::cached(
            painter,
            egui::Id::new(("stage-chain-card", index)),
            card,
            (
                alpha.surface.color,
                alpha.ground.color,
                alpha.edge.color,
                family_ink,
            ),
            |out| {
                chrome::panel_variant(
                    out,
                    card,
                    Some(alpha.surface.color),
                    alpha.ground.color,
                    Some((Weight::Hair, alpha.edge.color)),
                    index as u8,
                );
                chrome::trace(
                    out,
                    &[
                        egui::pos2(card.left() + chrome::CHAMFER, head.bottom()),
                        egui::pos2(card.right() - chrome::CHAMFER, head.bottom()),
                    ],
                    Weight::Hair,
                    alpha.edge.color,
                );
                for point in [
                    egui::pos2(card.center().x, card.top()),
                    egui::pos2(card.center().x, card.bottom()),
                    egui::pos2(card.left(), head.bottom() - 5.0),
                    egui::pos2(card.right(), head.bottom() - 5.0),
                ] {
                    chrome::pad(out, point, chrome::PAD, family_ink, true);
                }
                Sign::Seal(crate::ui::stage::browser::family_mark(column.family)).paint(
                    out,
                    egui::Rect::from_center_size(
                        egui::pos2(head.left() + 16.0, head.center().y),
                        egui::Vec2::splat(19.0),
                    ),
                    Weight::Hair,
                    family_ink,
                );
                chrome::pad(
                    out,
                    egui::pos2(head.right() - 11.0, head.center().y),
                    chrome::PAD + 1.0,
                    family_ink,
                    !column.bypassed,
                );
            },
        );

        // The full catalog name remains the column's semantic title; the
        // header cuts its stable target prefix. That address is the terse
        // machine name the narrow card can carry at the real block-face
        // size (POLY, SAT, REVERB), rather than shrinking prose into an
        // unreadable seven-pixel imitation of the face.
        let title = column.code.to_ascii_uppercase();
        painter.text(
            egui::pos2(head.left() + 30.0, head.top() + 5.0),
            egui::Align2::LEFT_TOP,
            &title,
            egui::FontId::monospace(11.0),
            family_ink,
        );
        if let Some(sample) = &column.sample {
            painter.text(
                egui::pos2(head.left() + 30.0, head.bottom() - 5.0),
                egui::Align2::LEFT_BOTTOM,
                fit_cells(sample, 24),
                row_font.clone(),
                family_ink,
            );
        }

        // The card's family word climbs its outer rail in the typewriter
        // hand, separate from the parameter-family signs on the next rail.
        let family_word = device_family_word(column.family);
        let galley =
            painter.layout_no_wrap(family_word.to_owned(), row_font.clone(), alpha.edge.color);
        painter.add(egui::Shape::Text(
            egui::epaint::TextShape::new(
                egui::pos2(card.left() + 5.0, card.bottom() - 8.0),
                galley,
                alpha.edge.color,
            )
            .with_angle(-core::f32::consts::FRAC_PI_2),
        ));

        let visible = row_offset..(row_offset + rows_shown).min(column.rows.len());
        for (family, run) in chain::family_runs(&column.rows) {
            let start = run.start.max(visible.start);
            let end = run.end.min(visible.end);
            if start >= end {
                continue;
            }
            let first_line = start - row_offset;
            let last_line = end - 1 - row_offset;
            let x = card.left() + 23.0;
            let y0 = body_top + first_line as f32 * pitch + pitch * 0.5;
            let y1 = body_top + last_line as f32 * pitch + pitch * 0.5;
            let mut shapes = Vec::new();
            chrome::rail(
                &mut shapes,
                egui::pos2(x, y0),
                egui::pos2(x, y1.max(y0 + 1.0)),
                &[0.0, 1.0],
                alpha.edge.color,
            );
            Sign::Family(family).paint(
                &mut shapes,
                egui::Rect::from_center_size(egui::pos2(x, y0), egui::Vec2::splat(11.0)),
                Weight::Hair,
                alpha.edge.color,
            );
            for shape in shapes {
                painter.add(shape);
            }
        }

        let cell_w = painter
            .layout_no_wrap("M".to_owned(), row_font.clone(), alpha.ink.color)
            .rect
            .width()
            .max(1.0);
        for line in 0..rows_shown {
            let Some(row) = column.rows.get(row_offset + line) else {
                break;
            };
            // The row: in from the family rail, and short of the right
            // edge by a real margin, so the value never touches the
            // casing and the cursor's brackets have room to sit.
            let rect = egui::Rect::from_min_size(
                egui::pos2(card.left() + 36.0, body_top + line as f32 * pitch),
                egui::vec2(card.width() - 48.0, pitch),
            );
            let on_row = cursor == Some((index, row_offset + line));
            if on_row {
                // The cursor row is the brightest thing on the card:
                // ground-coloured words on the focus ink, so the row
                // under the hand is never the hardest one to read.
                painter.rect_filled(rect, 0.0, alpha.live_dim.color);
                crate::ui::nav_cursor::claim(
                    painter,
                    ("stage-chain-row-cursor", index, row_offset + line),
                    rect,
                    crate::ui::nav_cursor::Kind::Row,
                    crate::ui::nav_cursor::Layer::Surface,
                    palette::colours().alert,
                );
            }
            // Read at the ink, not the edge: a card is a table to be
            // read, and a table in the structure rung is a table you
            // lean into. A value moved off its default steps up once
            // more, to the focus ink, so the edits are found at a glance.
            let value_ink = if on_row {
                palette::colours().bright
            } else if row.edited {
                alpha.focus.color
            } else {
                alpha.ink.color
            };
            let name_ink = if on_row {
                palette::colours().bright
            } else {
                alpha.ink.color
            };
            let value_w = painter
                .layout_no_wrap(row.value.clone(), row_font.clone(), value_ink)
                .rect
                .width();
            // A value column wide enough for the longest word a row can
            // say, so the gauges line up down the card instead of
            // wandering with each value's length.
            let value_col = (cell_w * 8.0).max(value_w);
            let gauge_w = 44.0;
            let value_x = rect.right();
            let gauge = egui::Rect::from_center_size(
                egui::pos2(value_x - value_col - gauge_w * 0.5 - 10.0, rect.center().y),
                egui::vec2(gauge_w, 7.0),
            );
            let name_room = (gauge.left() - rect.left() - 8.0).max(cell_w);
            let name_cells = (name_room / cell_w).floor().max(1.0) as usize;
            painter.text(
                egui::pos2(rect.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                fit_cells(&row.name, name_cells),
                row_font.clone(),
                name_ink,
            );
            let mut shapes = Vec::new();
            if row.choices > 0 {
                chrome::choice_bar(
                    &mut shapes,
                    gauge,
                    row.choices,
                    row.choice,
                    value_ink,
                    if on_row {
                        alpha.surface.color
                    } else {
                        alpha.edge.color
                    },
                );
            } else {
                chrome::tick_bar(
                    &mut shapes,
                    gauge,
                    12,
                    row.place,
                    value_ink,
                    if on_row {
                        alpha.surface.color
                    } else {
                        alpha.edge.color
                    },
                    true,
                );
            }
            for shape in shapes {
                painter.add(shape);
            }
            painter.text(
                egui::pos2(value_x, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &row.value,
                row_font.clone(),
                value_ink,
            );
        }

        if column.rows.len() > row_offset + rows_shown {
            let mut shapes = Vec::new();
            chrome::annotation_arrow(
                &mut shapes,
                egui::pos2(card.center().x, card.bottom() - 2.0),
                egui::pos2(card.center().x, card.bottom() + 8.0),
                alpha.ink.color,
            );
            for shape in shapes {
                painter.add(shape);
            }
        }
    }

    /// The send loom: the cable that makes a send a PATH rather than a
    /// number on a strip.
    ///
    /// Every channel's send rail is the same rail — the one that runs to
    /// that return — so the mixer stitches them together across the gaps
    /// between the strips and carries the line on to the return's own
    /// column. Each return keeps its colour, the same one the band's
    /// loom uses, so a send followed by eye in one view is the same
    /// send in the other. A segment leaving an open send is bright; one
    /// leaving a closed send is the ghost of where it could go.
    #[allow(clippy::too_many_arguments)]
    fn draw_send_loom(
        &self,
        painter: &egui::Painter,
        field: egui::Rect,
        bottom: f32,
        gap: f32,
        inner: f32,
        channels: &[crate::ui::stage::mixer::Channel],
        returns: &[(usize, egui::Rect)],
    ) {
        let tracks = self.shown_tracks(field.width());
        if tracks.is_empty() || returns.is_empty() {
            return;
        }
        let alpha = self.glass();
        let count = self
            .song
            .console
            .aux
            .len()
            .min(crate::sequencing::ReturnTrack::MAX);
        let strips: Vec<(usize, egui::Rect)> = tracks
            .clone()
            .enumerate()
            .map(|(slot, track)| {
                (
                    track,
                    mixer::strip_beneath(heads::head_rect(field, slot), bottom, gap),
                )
            })
            .collect();
        let mut shapes = Vec::new();
        for (slot, ret_strip) in returns {
            let Some(&ink) = RETURN_INK.get(*slot) else {
                continue;
            };
            let y = strips
                .first()
                .map(|(_, strip)| mixer::send_y(*strip, inner, count, true, *slot));
            let Some(y) = y else { continue };
            // Across every gap between the strips, and on to the
            // return's own column.
            let mut runs: Vec<(f32, f32, f32)> = Vec::new();
            for pair in strips.windows(2) {
                let ((left_track, left), (_, right)) = (pair[0], pair[1]);
                let open = channels
                    .get(left_track)
                    .and_then(|channel| channel.sends[*slot])
                    .unwrap_or(0.0);
                runs.push((left.right(), right.left(), open));
            }
            if let Some((last_track, last)) = strips.last() {
                let open = channels
                    .get(*last_track)
                    .and_then(|channel| channel.sends[*slot])
                    .unwrap_or(0.0);
                runs.push((last.right(), ret_strip.left(), open));
            }
            for (from, to, open) in runs {
                if to <= from {
                    continue;
                }
                chrome::trace(
                    &mut shapes,
                    &[egui::pos2(from, y), egui::pos2(to, y)],
                    Weight::Hair,
                    tint(ink, 0.22 + 0.78 * open.clamp(0.0, 1.0)),
                );
            }
            // Where each channel taps the line: a pad on its own mark,
            // filled when it is sending.
            for (track, strip) in &strips {
                let Some(open) = channels
                    .get(*track)
                    .and_then(|channel| channel.sends[*slot])
                else {
                    continue;
                };
                chrome::pad(
                    &mut shapes,
                    egui::pos2(mixer::send_x(*strip, inner, open), y),
                    chrome::PAD - 1.0,
                    tint(ink, 0.3 + 0.7 * open),
                    open > 0.005,
                );
            }
            // And where it lands: the return's own column, tapped on its
            // wall so the cable plainly arrives somewhere.
            chrome::pad(
                &mut shapes,
                egui::pos2(ret_strip.left(), y),
                chrome::PAD,
                ink,
                true,
            );
            chrome::trace(
                &mut shapes,
                &[
                    egui::pos2(ret_strip.left(), y),
                    egui::pos2(ret_strip.left() + 8.0, y),
                ],
                Weight::Heavy,
                ink,
            );
        }
        let _ = alpha;
        painter.extend(shapes);
    }
}
