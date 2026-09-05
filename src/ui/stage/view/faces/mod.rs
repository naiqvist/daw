//! The console's faces: one file per section, one vector instrument
//! per parameter.
//!
//! A card's glass is not a table of names and numbers. It is the
//! section itself, drawn: the filter's own response, the compressor's
//! own decision, the delay's own taps, each one a thing the hand
//! moves. A user who has never read a manual should know what a
//! control does by watching it move.
//!
//! Two rules hold the whole band together.
//!
//! **Every control is vector art you MOVE.** No boxes with text
//! labels; no number as the primary readout. A word may sit on a face
//! as an engraving, never as the answer.
//!
//! **Every animation is a real result of the DSP.** Nothing here
//! breathes from a wall clock. It moves because [`Face::said`] — the
//! section's own telemetry, measured in the audio callback and handed
//! across — said so, because the transport's [`Phase`] said so, or
//! because the hand moved a value. A curve is drawn from the section's
//! own coefficients, never from a sketch of what the filter probably
//! does.
//!
//! A face owns NO state: a card is rebuilt from engine units every
//! frame, so everything it needs arrives in [`Face`] and everything it
//! remembers is an egui animation keyed by the piece's index.

use super::*;
use crate::console::{SectionKind, SectionParams, Telemetry};
// The piece's geometry is the strip's; a face borrows it rather than
// re-deriving it, so the drawing and the casing can never disagree.
use super::strip::{
    self, FIGURE_MAX_H, HEAD_H, JOINT_H, Piece, TONGUE, bay_rect, on_arc, plinth_rect,
    preamp_pivot, recess_of, width_of,
};

pub mod tool;

mod ceiling;
mod cut;
mod door;
mod drift;
mod drive;
mod echo;
mod four;
mod glue;
mod grit;
mod hit;
mod iron;
mod out;
mod phase;
mod preamp;
mod pump;
mod ring;
mod room;
mod scope;
mod shadow;
mod shine;
mod smear;
mod spectra;
mod split;
mod tape;
mod tone;
mod vca;

/// Everything a face may see, built fresh by the strip every frame.
///
/// A face reads this and nothing else: not the stage, not the song,
/// not the engine. That is what keeps a card honest — it can only draw
/// what the desk has already measured or the hand has already set.
pub struct Face<'a> {
    /// Already clipped to the glass and to the piece's crevices.
    pub painter: &'a egui::Painter,
    /// Which piece this is, where it stands, and its joints.
    pub piece: Piece,
    /// The screen the face owns. Crevices are extra, see [`Face::bay`].
    pub glass: egui::Rect,
    pub alpha: &'static design::Alphabet,
    /// The transport, for anything that runs on the beat.
    pub phase: Phase,
    /// What the section measured of itself, last block.
    pub said: Telemetry,
    /// The channel's peak, linear, when the band has one.
    pub level: Option<f32>,
    /// The parameter the keyboard is standing on, by table index.
    pub selected: Option<usize>,
    /// The section is switched out. The face still draws: the strip
    /// veils it and writes the word over the top.
    pub out: bool,
    /// The section's settings, defaults filled in — exactly what the
    /// green-side curve readers in [`crate::console`] want.
    pub params: SectionParams,
}

impl Face<'_> {
    pub fn kind(&self) -> SectionKind {
        self.piece.kind
    }

    /// A parameter's value in engine units.
    pub fn value(&self, param: u32) -> f32 {
        self.params.value(param)
    }

    /// A parameter's place across its own range, 0..1.
    pub fn place(&self, param: u32) -> f32 {
        let Some(def) = self.kind().table().iter().find(|def| def.id == param) else {
            return 0.0;
        };
        let span = def.max - def.min;
        if span.abs() < f32::EPSILON {
            return 0.0;
        }
        ((self.value(param) - def.min) / span).clamp(0.0, 1.0)
    }

    /// A parameter's place about its own centre, −1..1, for anything
    /// bipolar. Unipolar parameters read 0 at their floor.
    pub fn swing(&self, param: u32) -> f32 {
        let Some(def) = self.kind().table().iter().find(|def| def.id == param) else {
            return 0.0;
        };
        if def.min < 0.0 && def.max > 0.0 {
            let reach = if self.value(param) >= 0.0 {
                def.max
            } else {
                -def.min
            };
            (self.value(param) / reach.max(f32::EPSILON)).clamp(-1.0, 1.0)
        } else {
            self.place(param)
        }
    }

    /// The keyboard is standing on this parameter right now.
    pub fn awake(&self, param: u32) -> bool {
        self.selected == Some(param as usize)
    }

    /// How awake, eased: 0 at rest, 1 under the hand. An instrument is
    /// dim and small until the hand arrives, then bright — which is
    /// what lets a face carry a dozen controls without shouting.
    pub fn lit(&self, param: u32) -> f32 {
        self.painter.ctx().animate_bool_with_time(
            egui::Id::new(("stage-face-lit", self.piece.index, param)),
            self.awake(param),
            0.11,
        )
    }

    /// A value that walks to where it is going rather than jumping.
    pub fn anim(&self, tag: &'static str, value: f32, seconds: f32) -> f32 {
        self.painter.ctx().animate_value_with_time(
            egui::Id::new(("stage-face-anim", self.piece.index, tag)),
            value,
            seconds,
        )
    }

    pub fn ink(&self) -> egui::Color32 {
        self.alpha.ink.color
    }

    pub fn edge(&self) -> egui::Color32 {
        self.alpha.edge.color
    }

    pub fn ground(&self) -> egui::Color32 {
        self.alpha.ground.color
    }

    pub fn well(&self) -> egui::Color32 {
        self.alpha.well.color
    }

    pub fn focus(&self) -> egui::Color32 {
        self.alpha.focus.color
    }

    /// The live ink, pulsing with the beat while the transport rolls.
    pub fn live(&self) -> egui::Color32 {
        motion::pulse_ink(self.alpha.live.color, self.alpha.live_dim.color, self.phase)
    }

    pub fn hot(&self) -> egui::Color32 {
        self.alpha.jeopardy_active.color
    }

    /// The channel's peak in dBFS, floored where a meter stops caring.
    pub fn level_db(&self) -> f32 {
        match self.level {
            Some(peak) if peak > 1e-6 => 20.0 * peak.log10(),
            _ => -72.0,
        }
    }

    /// The crevice cut into the right wall, when the piece has one.
    pub fn bay(&self) -> Option<egui::Rect> {
        strip::bay_rect(self.piece.rect, self.piece.kind)
    }

    /// The low shelf at the foot, when the piece has one.
    pub fn plinth(&self) -> Option<egui::Rect> {
        strip::plinth_rect(self.piece.rect, self.piece.kind)
    }

    /// The font a face engraves with. Engravings only: a face never
    /// answers a question with a number.
    pub fn font(&self) -> egui::FontId {
        egui::FontId::monospace(design::px(design::type_scale::MICRO))
    }

    /// Put the application's one cursor on the instrument the keyboard
    /// is addressing. The mark is not a row under the drawing: it is
    /// four bright corners just inside the instrument itself.
    pub fn mark(&self, lay: &impl Layout) {
        self.mark_signed(lay, crate::ui::nav_cursor::Signature::Plain);
    }

    /// The same, but the mark BECOMES the instrument it is standing on:
    /// squeezed by the reduction, opened by the gate, coloured by the
    /// band, swept by the modulator. Every signature carries a measured
    /// figure — see [`crate::ui::nav_cursor::Signature`] — so a cursor
    /// that is moving is a cursor reading something.
    pub fn mark_signed(&self, lay: &impl Layout, signature: crate::ui::nav_cursor::Signature) {
        let Some(param) = self.selected else {
            return;
        };
        let Some(rect) = lay.control(param) else {
            return;
        };
        crate::ui::nav_cursor::claim_signed(
            self.painter,
            ("stage-face-cursor", self.piece.index),
            rect,
            lay.cursor_kind(param),
            crate::ui::nav_cursor::Layer::Surface,
            self.focus(),
            signature,
        );
    }

    /// Which cell of `of` the transport is inside, this beat. The one
    /// place a face may read the clock, and it is the transport's clock.
    pub fn beat_cell(&self, of: usize) -> crate::ui::nav_cursor::Signature {
        let of = of.max(1);
        crate::ui::nav_cursor::Signature::Beat {
            cell: ((self.phase.beat * of as f32) as usize).min(of - 1),
            of,
        }
    }
}

/// A face's layout: where every parameter's instrument stands.
///
/// Authored as one table so the key that addresses a parameter and the
/// art that answers cannot drift apart, and so a test can assert that
/// every parameter in the section's table has exactly one place on the
/// glass.
pub trait Layout {
    /// Every parameter, by id, and the rectangle its instrument owns.
    fn controls(&self) -> Vec<(u32, egui::Rect)>;

    fn control(&self, param: usize) -> Option<egui::Rect> {
        self.controls()
            .into_iter()
            .find_map(|(id, rect)| (id as usize == param).then_some(rect))
    }

    /// The shape the cursor takes on this instrument. Most are
    /// instruments; a long rail reads better as a row, a tall one as a
    /// column.
    fn cursor_kind(&self, _param: usize) -> crate::ui::nav_cursor::Kind {
        crate::ui::nav_cursor::Kind::Instrument
    }
}

/// The section's figure on its glass.
pub fn draw(face: &Face<'_>) {
    match face.piece.kind {
        SectionKind::Preamp => preamp::draw(face),
        SectionKind::Tone => tone::draw(face),
        SectionKind::Door => door::draw(face),
        SectionKind::Cut => cut::draw(face),
        SectionKind::Hit => hit::draw(face),
        SectionKind::Four => four::draw(face),
        SectionKind::Vca => vca::draw(face),
        SectionKind::Split => split::draw(face),
        SectionKind::Pump => pump::draw(face),
        SectionKind::Drive => drive::draw(face),
        SectionKind::Grit => grit::draw(face),
        SectionKind::Shine => shine::draw(face),
        SectionKind::Drift => drift::draw(face),
        SectionKind::Phase => phase::draw(face),
        SectionKind::Smear => smear::draw(face),
        SectionKind::Ring => ring::draw(face),
        SectionKind::Spectra => spectra::draw(face),
        SectionKind::Echo => echo::draw(face),
        SectionKind::Room => room::draw(face),
        SectionKind::Out => out::draw(face),
        SectionKind::Glue => glue::draw(face),
        SectionKind::Iron => iron::draw(face),
        SectionKind::Ceiling => ceiling::draw(face),
        SectionKind::Scope => scope::draw(face),
        SectionKind::Tape => tape::draw(face),
        SectionKind::Shadow => shadow::draw(face),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A glass of the size a piece of `kind` really gets, for a layout
    /// test to lay itself out on.
    pub(super) fn glass_of(kind: SectionKind) -> (egui::Rect, egui::Rect) {
        let piece = egui::Rect::from_min_size(
            egui::pos2(40.0, 60.0),
            egui::vec2(strip::width_of(kind), 250.0),
        );
        (piece, strip::recess_of(piece, kind).shrink(3.0))
    }

    /// The desk's standing promise, checked for every section that has
    /// drawn its face: one parameter, one instrument, no exceptions and
    /// no leftovers.
    pub(super) fn one_instrument_each(kind: SectionKind, lay: &impl Layout) {
        let controls = lay.controls();
        for def in kind.table() {
            let rect = lay
                .control(def.id as usize)
                .unwrap_or_else(|| panic!("{:?}: {} has no instrument", kind, def.name));
            assert!(rect.is_positive(), "{:?}: {} has no room", kind, def.name);
        }
        assert_eq!(
            controls.len(),
            kind.table().len(),
            "{kind:?} has {} instruments for {} parameters",
            controls.len(),
            kind.table().len()
        );
        let mut ids: Vec<u32> = controls.iter().map(|(id, _)| *id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), controls.len(), "{kind:?} drew a parameter twice");
    }

    /// Every instrument stands inside the piece it belongs to. The
    /// glass is the usual home; a crevice is the other one, which is
    /// the whole point of cutting a crevice.
    pub(super) fn all_inside_the_piece(kind: SectionKind, piece: egui::Rect, lay: &impl Layout) {
        for (id, rect) in lay.controls() {
            assert!(
                piece.expand(strip::TONGUE).contains_rect(rect),
                "{kind:?}: parameter {id} left the piece"
            );
        }
    }
}
