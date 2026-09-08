//! Euclidean rhythms, spoken as a mode of the step grid.
//!
//! E on the grid enters the mode: the pattern's lit steps become the
//! hit count, and the grid is rewritten to the Euclidean rhythm with
//! that many hits over the clip's steps. Up and Down add and remove
//! hits, Left and Right turn the rhythm, and every change is laid on
//! the pattern as it is made, so the ear hears what the arrows do.
//! Enter keeps what is there; Escape puts back what was there before.
//!
//! The rhythm is Bjorklund's — the hits as evenly spread as `n` cells
//! allow — by the Bresenham reading of it: step `i` is a hit when the
//! line `i·k/n` crosses an integer there. Rotation turns the result.

use crate::ui::sequencer::grammar::Motion;

/// The steps a rhythm of `hits` over `steps` lights, turned `rotation`
/// steps later. Zero hits is silence; hits past the steps are every
/// step.
///
/// Bjorklund's algorithm as written: the hits and the rests as two
/// piles of sequences, the shorter pile paired onto the longer until
/// one sequence or none remains to pair, then read out in order. It
/// gives the forms the books print — `x.xx.xx.` for five in eight —
/// rather than a rotation of them.
pub fn rhythm(hits: usize, steps: usize, rotation: usize) -> Vec<bool> {
    let n = steps.max(1);
    let k = hits.min(n);
    let mut heads: Vec<Vec<bool>> = vec![vec![true]; k];
    let mut tails: Vec<Vec<bool>> = vec![vec![false]; n - k];
    while tails.len() > 1 && !heads.is_empty() {
        let pairs = heads.len().min(tails.len());
        let rest: Vec<Vec<bool>> = tails.split_off(pairs);
        for (head, tail) in heads.iter_mut().zip(tails.drain(..)) {
            head.extend(tail);
        }
        let leftover_heads: Vec<Vec<bool>> = if heads.len() > pairs {
            heads.split_off(pairs)
        } else {
            Vec::new()
        };
        tails = if rest.is_empty() {
            leftover_heads
        } else {
            rest
        };
    }
    let plain: Vec<bool> = heads.into_iter().chain(tails).flatten().take(n).collect();
    let plain = if plain.len() == n {
        plain
    } else {
        (0..n).map(|i| (i * k) % n < k).collect()
    };
    let r = rotation % n;
    (0..n).map(|i| plain[(i + n - r) % n]).collect()
}

/// The mode while it is on: what the rhythm is, what the grid had
/// before, and what the grid holds now.
#[derive(Clone, Debug, PartialEq)]
pub struct Euclid {
    pub steps: usize,
    pub hits: usize,
    pub rotation: usize,
    /// The lit steps when the mode was entered — what Escape restores.
    pub before: Vec<bool>,
    /// The lit steps as the mode has laid them so far.
    pub current: Vec<bool>,
}

impl Euclid {
    /// Enter on a grid whose lit steps are `lit`: as many hits as were
    /// lit, unturned.
    pub fn enter(lit: Vec<bool>) -> Self {
        let steps = lit.len().max(1);
        let hits = lit.iter().filter(|on| **on).count();
        Self {
            steps,
            hits,
            rotation: 0,
            before: lit.clone(),
            current: lit,
        }
    }

    /// The rhythm as it stands.
    pub fn target(&self) -> Vec<bool> {
        rhythm(self.hits, self.steps, self.rotation)
    }

    /// An arrow: Up and Down are hits, Left and Right the turn, `count`
    /// at a time. Hits stop at the ends; the turn wraps.
    pub fn adjust(&mut self, motion: Motion, count: usize) {
        let count = count.max(1);
        match motion {
            Motion::Up => self.hits = (self.hits + count).min(self.steps),
            Motion::Down => self.hits = self.hits.saturating_sub(count),
            Motion::Right => self.rotation = (self.rotation + count) % self.steps,
            Motion::Left => {
                self.rotation = (self.rotation + self.steps - count % self.steps) % self.steps
            }
        }
    }

    /// The line over the grid while the mode is on.
    pub fn hud(&self) -> String {
        format!(
            "EUCLID {}/{} · TURN {}   ↑↓ HITS  ←→ TURN  ENTER KEEP  ESC BACK",
            self.hits, self.steps, self.rotation
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(lit: &[bool]) -> String {
        lit.iter().map(|on| if *on { 'x' } else { '.' }).collect()
    }

    #[test]
    fn the_classics_come_out_as_the_books_write_them() {
        assert_eq!(word(&rhythm(4, 16, 0)), "x...x...x...x...");
        assert_eq!(word(&rhythm(3, 8, 0)), "x..x..x.");
        assert_eq!(word(&rhythm(5, 8, 0)), "x.xx.xx.");
        assert_eq!(word(&rhythm(5, 16, 0)), "x..x..x..x..x...");
        assert_eq!(word(&rhythm(0, 8, 0)), "........");
        assert_eq!(word(&rhythm(8, 8, 0)), "xxxxxxxx");
        assert_eq!(word(&rhythm(9, 8, 0)), "xxxxxxxx");
        assert_eq!(rhythm(3, 0, 0).len(), 1);
    }

    #[test]
    fn a_turn_moves_the_hits_later_and_wraps() {
        assert_eq!(word(&rhythm(3, 8, 1)), ".x..x..x");
        assert_eq!(word(&rhythm(3, 8, 8)), word(&rhythm(3, 8, 0)));
        assert_eq!(word(&rhythm(4, 16, 2)), "..x...x...x...x.");
    }

    #[test]
    fn the_mode_starts_from_what_was_lit_and_the_arrows_move_it() {
        let lit = vec![true, false, true, false, false, false, false, false];
        let mut e = Euclid::enter(lit.clone());
        assert_eq!((e.steps, e.hits, e.rotation), (8, 2, 0));
        assert_eq!(e.before, lit);
        assert_eq!(word(&e.target()), "x...x...");
        e.adjust(Motion::Up, 1);
        assert_eq!(word(&e.target()), "x..x..x.");
        e.adjust(Motion::Right, 2);
        assert_eq!(word(&e.target()), "x.x..x..");
        e.adjust(Motion::Left, 3);
        assert_eq!(e.rotation, 7);
        e.adjust(Motion::Down, 9);
        assert_eq!(e.hits, 0);
        e.adjust(Motion::Up, 99);
        assert_eq!(e.hits, 8);
        assert!(e.hud().starts_with("EUCLID 8/8 · TURN 7"));
    }
}
