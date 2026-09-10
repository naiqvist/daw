//! Explicit navigation by musical identity, independent of the old cursor.
use super::{Stage, transport::Place};
use crate::sequencing::Song;

pub(super) const COMMANDS: &[crate::ui::palette::TypedCommand] = &[
    crate::ui::palette::TypedCommand {
        name: "go",
        usage: "go track <name|#stable-id> | go clip <name|#stable-id> | go bar <1-based> | go tick <0-based> · moves cursor, not playhead",
    },
];

fn named<'a>(
    target: &str,
    items: impl Iterator<Item = (usize, u64, &'a str)>,
) -> Result<usize, String> {
    let target = target.trim().trim_matches('"');
    let id = target
        .strip_prefix('#')
        .map(|s| s.parse::<u64>().map_err(|_| "invalid stable id"))
        .transpose()?;
    let matches: Vec<_> = items
        .filter(|(_, candidate, name)| {
            id.map_or_else(|| name.eq_ignore_ascii_case(target), |id| id == *candidate)
        })
        .collect();
    match matches.as_slice() {
        [(index, _, _)] => Ok(*index),
        [] => Err(format!("no object named {target}")),
        _ => Err(format!(
            "ambiguous {target}; use {}",
            matches
                .iter()
                .map(|(_, id, _)| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(" or ")
        )),
    }
}

/// Invert the existing meter-aware readout with a bounded binary search.
/// No bar-by-bar walking even for distant cursor jumps.
fn bar_tick(song: &Song, bar: usize) -> Result<usize, String> {
    if !(1..=1_000_000).contains(&bar) {
        return Err("bar must be 1..1000000".into());
    }
    let (mut low, mut high) = (0usize, 1_000_000_000usize);
    if Place::of(song, high).bar < bar {
        return Err("bar exceeds supported song time".into());
    }
    while low < high {
        let middle = low + (high - low) / 2;
        if Place::of(song, middle).bar < bar {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    Ok(low)
}

impl Stage {
    fn navigate_command(&mut self, input: &str) -> Result<(), String> {
        let mut words = input.splitn(3, char::is_whitespace);
        let _ = words.next();
        let kind = words.next().unwrap_or_default();
        let target = words.next().unwrap_or_default().trim();
        if target.is_empty() {
            return Err("give a destination".into());
        }
        if self.steps_checkpoint.is_some()
            || self.sample.is_some()
            || self.forge.is_some()
            || self.lab.open
            || self.utility.page.is_some()
            || self.renaming.is_some()
            || self.modulation.is_some()
            || self.plock_editor.is_some()
            || self.trig_menu.is_some()
        {
            return Err("finish or close the current editor before navigating".into());
        }
        let current = self.deck_track().unwrap_or(0);
        let (track, tick, open) = match kind {
            "track" => (
                named(
                    target,
                    self.song
                        .tracks
                        .iter()
                        .enumerate()
                        .map(|(i, t)| (i, t.id.0, t.name.as_str())),
                )?,
                self.arrangement.tick,
                false,
            ),
            "clip" => {
                let at = named(
                    target,
                    self.song
                        .patterns
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (i, p.id.0, p.name.as_str())),
                )?;
                let pattern = self.song.patterns[at].id;
                let owners: Vec<_> = self
                    .song
                    .tracks
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| {
                        t.blocks
                            .iter()
                            .filter(|b| b.pattern_id == pattern)
                            .min_by_key(|b| b.start_tick)
                            .map(|b| (i, b.start_tick))
                    })
                    .collect();
                match owners.as_slice() {
                    [(track, tick)] => (*track, *tick, true),
                    [] => return Err("clip has no arrangement instance; place it first".into()),
                    _ => return Err(
                        "clip is shared across tracks; navigate to the track and open its block"
                            .into(),
                    ),
                }
            }
            "bar" => (
                current,
                bar_tick(&self.song, target.parse().map_err(|_| "invalid bar")?)?,
                false,
            ),
            "tick" => {
                let tick = target
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n <= 1_000_000_000)
                    .ok_or("tick must be 0..1000000000")?;
                (current, tick, false)
            }
            _ => return Err("go track, clip, bar or tick".into()),
        };
        if track >= self.song.tracks.len() {
            return Err("track missing".into());
        }
        // Nothing before this point changed UI or Song. Selection is transient,
        // and must not turn Enter's navigation into an arrangement replacement.
        while self.focus.escape() {}
        self.inside = None;
        self.steps = None;
        self.deck.open = false;
        self.deck.hero_focus = None;
        self.browser = None;
        self.chain = None;
        self.mixing = false;
        self.song_view = true;
        self.arrangement.clear_selection();
        self.arrangement.hold = None;
        self.arrangement.track = track;
        self.arrangement.tick = tick;
        self.arrangement.fit(&self.song);
        if open {
            self.song_enter().map_err(|_| "could not open clip")?;
        }
        self.notice = Some(format!(
            "go · track #{} {} · cursor {} · tick {}{}",
            self.song.tracks[track].id.0,
            self.song.tracks[track].name,
            Place::of(&self.song, tick).readout(),
            tick,
            if open { " · clip open" } else { "" }
        ));
        Ok(())
    }

    pub(super) fn apply_navigation_command(&mut self, input: &str) -> bool {
        match self.navigate_command(input) {
            Ok(()) => true,
            Err(error) => {
                self.notice = Some(format!("REFUSED go · {error}"));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_survives_reorder_and_navigation_does_not_edit_or_seek() {
        let mut s = Stage::new();
        let id = s.song.tracks[0].id;
        let clip = s.song.tracks[0].blocks[0].pattern_id;
        s.song.add_track(crate::sequencing::TrackKind::Instrument);
        s.song.tracks.swap(0, 1);
        let original = s.song.clone();
        assert!(s.apply_timeline_command(&format!("go track #{}", id.0)));
        assert_eq!(s.arrangement.track, 1);
        assert!(s.apply_timeline_command("go bar 5"));
        assert_eq!(s.arrangement.tick, 768);
        assert_eq!(s.transport.tick(), 0);
        assert!(s.apply_timeline_command(&format!("go clip #{}", clip.0)));
        assert_eq!(s.inside.unwrap().pattern, clip);
        assert_eq!(s.song, original);
        let cursor = s.arrangement.clone();
        assert!(!s.apply_timeline_command("go track #999999"));
        assert_eq!(s.arrangement, cursor);
        assert_eq!(s.song, original);
    }

    #[test]
    fn bars_use_meter_changes_and_duplicate_names_refuse() {
        let mut s = Stage::new();
        s.song.set_meter_mark(0, 7, 8);
        assert_eq!(bar_tick(&s.song, 3).unwrap(), 336);
        s.song.set_meter_mark(336, 4, 4);
        assert_eq!(bar_tick(&s.song, 4).unwrap(), 528);
        s.song.add_track(crate::sequencing::TrackKind::Instrument);
        s.song.tracks[0].name = "drums".into();
        s.song.tracks[1].name = "drums".into();
        assert!(!s.apply_timeline_command("go track drums"));
        assert!(s.notice.as_deref().unwrap().contains("ambiguous"));
    }
}
