use super::*;
use crate::{midi_lab::Voice, theory};

pub fn compose(
    c: &Composition,
    events: &[NoteEvent],
) -> Result<(Vec<NoteEvent>, Vec<Finding>), String> {
    let spec = &c.voices[4];
    let rules = &spec.counter;
    if rules.species == Species::Off {
        return Ok((vec![], vec![]));
    }
    if rules.leader == Voice::Counterpoint {
        return Err("Counterpoint cannot lead itself".into());
    }
    let mut leader = events
        .iter()
        .filter(|n| n.voice == rules.leader)
        .collect::<Vec<_>>();
    leader.sort_by_key(|n| (n.start, n.id));
    if leader.is_empty() {
        return Err("Counterpoint needs notes in its selected leader".into());
    }
    if leader.windows(2).any(|pair| pair[0].end() > pair[1].start) {
        return Err("The counterpoint leader must be monophonic".into());
    }
    let mut result: Vec<NoteEvent> = Vec::new();
    let mut findings = Vec::new();
    if rules.species == Species::Canon {
        for n in &leader {
            let start = n
                .start
                .checked_add(rules.delay)
                .ok_or("Canon delay overflows")?;
            if start
                .checked_add(n.length)
                .is_none_or(|end| end > MAX_TICKS)
            {
                return Err("The complete canon tail exceeds the timeline capacity".into());
            }
            let pitch = i32::from(n.pitch) + i32::from(rules.interval);
            if pitch < i32::from(spec.low) || pitch > i32::from(spec.high) {
                return Err("Literal canon exceeds the follower's range".into());
            }
            result.push(NoteEvent {
                id: identity(&[4, n.id, 0x63616e6f6e]),
                voice: Voice::Counterpoint,
                member: None,
                pitch: pitch as u8,
                start,
                length: n.length,
                velocity: spec.velocity,
                provenance: Provenance::new(
                    "counterpoint.canon",
                    c.harmony_at(start).map(|h| h.id),
                    format!(
                        "Literal canon {:+} semitones, {} ticks later",
                        rules.interval, rules.delay
                    ),
                ),
            });
        }
    } else {
        result = species(c, &leader)?;
    }
    if let Some(interval) = rules.invertible {
        if ![12, 16, 19].contains(&interval) {
            return Err("Invertible counterpoint supports octave, tenth or twelfth".into());
        }
        for note in &result {
            if let Some(lead) = leader
                .iter()
                .find(|lead| lead.start <= note.start && lead.end() > note.start)
            {
                let transformed = u16::from(note.pitch) + u16::from(interval);
                if transformed > 127 || !theory::is_consonant(transformed as u8, lead.pitch) {
                    findings.push(Finding {
                        rule: "counterpoint.invertible".into(),
                        events: vec![note.id, lead.id],
                        detail: format!(
                            "This interval is not consonant after inversion at {interval} semitones"
                        ),
                    });
                }
            }
        }
    }
    if rules.strict && !findings.is_empty() {
        return Err(findings[0].detail.clone());
    }
    Ok((result, findings))
}

/// A bounded beam of paths with shared back-pointers. No prefix note arrays
/// are cloned while searching, and the transition budget is explicit.
fn species(c: &Composition, leader: &[&NoteEvent]) -> Result<Vec<NoteEvent>, String> {
    let spec = &c.voices[4];
    let rules = &spec.counter;
    struct Slot {
        id: u64,
        start: u32,
        length: u32,
        upper: u8,
        harmony: Option<u64>,
        first: bool,
        last: bool,
    }
    #[derive(Clone, Copy)]
    struct Path {
        pitch: u8,
        upper: u8,
        cost: i64,
        back: Option<usize>,
    }
    struct Link {
        slot: usize,
        pitch: u8,
        previous: Option<usize>,
    }
    let mut slots = Vec::new();
    for (i, lead) in leader.iter().enumerate() {
        let divisions = if i + 1 == leader.len() {
            1
        } else {
            match rules.species {
                Species::First => 1,
                Species::Second | Species::Fourth => 2,
                Species::Third => 4,
                Species::Fifth => {
                    if i % 2 == 0 {
                        2
                    } else {
                        4
                    }
                }
                _ => 1,
            }
        };
        if lead.length % divisions != 0 {
            return Err("Species subdivision cannot be represented exactly; use an even, connected leader rhythm".into());
        }
        let length = lead.length / divisions;
        if length == 0 {
            return Err("Leader notes are too short for the selected species".into());
        }
        for part in 0..divisions {
            let start = lead.start + part * length;
            slots.push(Slot {
                id: identity(&[4, lead.id, u64::from(part), 0x73706563696573]),
                start,
                length,
                upper: lead.pitch,
                harmony: c.harmony_at(start).map(|h| h.id),
                first: i == 0 && part == 0,
                last: i + 1 == leader.len(),
            });
        }
    }
    let mut paths = vec![Path {
        pitch: 0,
        upper: 0,
        cost: 0,
        back: None,
    }];
    let mut history = Vec::new();
    let mut transitions = 0usize;
    for (index, slot) in slots.iter().enumerate() {
        let fixed = c
            .overrides
            .iter()
            .find(|o| o.id == slot.id && !o.deleted)
            .and_then(|o| o.pitch);
        let mut next = Vec::new();
        for pitch in spec.low..=spec.high {
            if fixed.is_some_and(|p| p != pitch)
                || pitch >= slot.upper
                || !theory::is_consonant(pitch, slot.upper)
            {
                continue;
            }
            if slot.first && ![0, 7].contains(&((slot.upper - pitch) % 12))
                || slot.last && (slot.upper - pitch) % 12 != 0
            {
                continue;
            }
            if c.key_at(slot.start).is_some_and(|key| !key.contains(pitch)) {
                continue;
            }
            if rules.invertible.is_some_and(|iv| {
                u16::from(pitch) + u16::from(iv) > 127
                    || !theory::is_consonant(pitch + iv, slot.upper)
            }) {
                continue;
            }
            let mut best = None;
            for path in &paths {
                transitions += 1;
                if transitions > 262144 {
                    return Err("Counterpoint exceeded 262144 transitions; compose a shorter phrase and repeat it through the form".into());
                }
                let movement = if path.back.is_none() {
                    i16::from(pitch) - i16::from(slot.upper.saturating_sub(12))
                } else {
                    i16::from(pitch) - i16::from(path.pitch)
                };
                let upper = i16::from(slot.upper) - i16::from(path.upper);
                if path.back.is_some() {
                    if movement.abs() > 12
                        || [6, 10, 11].contains(&movement.abs())
                        || theory::is_parallel_perfect(
                            (path.pitch, path.upper),
                            (pitch, slot.upper),
                        )
                    {
                        continue;
                    }
                    if movement != 0
                        && movement.signum() == upper.signum()
                        && upper.abs() > 2
                        && [0, 7].contains(&((slot.upper - pitch) % 12))
                    {
                        continue;
                    }
                }
                let cost = path.cost
                    + i64::from(movement.abs()) * 4
                    + i64::from(movement.abs().saturating_sub(5)).pow(2)
                    + if movement != 0 && movement.signum() == upper.signum() {
                        3
                    } else {
                        0
                    }
                    + if movement == 0 { 2 } else { 0 };
                let value = (cost, path.back);
                if best.is_none_or(|old| value < old) {
                    best = Some(value);
                }
            }
            if let Some((cost, back)) = best {
                next.push((cost, pitch, back));
            }
        }
        next.sort();
        next.truncate(16);
        if next.is_empty() {
            return Err(format!(
                "No contrapuntal path satisfies the range, locks and interval rules at tick {}",
                slot.start
            ));
        }
        paths.clear();
        for (cost, pitch, previous) in next {
            let back = history.len();
            history.push(Link {
                slot: index,
                pitch,
                previous,
            });
            paths.push(Path {
                pitch,
                upper: slot.upper,
                cost,
                back: Some(back),
            });
        }
    }
    let mut out = Vec::new();
    let mut back = paths.first().and_then(|p| p.back);
    while let Some(index) = back {
        let link = &history[index];
        let slot = &slots[link.slot];
        out.push(NoteEvent {
            id: slot.id,
            voice: Voice::Counterpoint,
            member: None,
            pitch: link.pitch,
            start: slot.start,
            length: slot.length,
            velocity: spec.velocity,
            provenance: Provenance::new(
                "counterpoint.species",
                slot.harmony,
                format!(
                    "{} · {} semitones below {} · beam 16 · {transitions} transitions",
                    rules.species.label(),
                    slot.upper - link.pitch,
                    rules.leader.label()
                ),
            ),
        });
        back = link.previous;
    }
    out.reverse();
    if rules.invertible.is_none()
        && matches!(
            rules.species,
            Species::Second | Species::Third | Species::Fifth
        )
    {
        for i in 1..out.len().saturating_sub(1) {
            let n = &out[i];
            if c.overrides.iter().any(|o| o.id == n.id) {
                continue;
            }
            let Some(lead) = leader
                .iter()
                .find(|lead| lead.start < n.start && lead.end() > n.start)
            else {
                continue;
            };
            let before = out[i - 1].pitch;
            let after = out[i + 1].pitch;
            let direction = (i16::from(after) - i16::from(before)).signum();
            if let Some(p) = c
                .key_at(n.start)
                .and_then(|k| k.step(before, direction).ok())
                .filter(|p| {
                    *p >= spec.low
                        && *p <= spec.high
                        && *p < lead.pitch
                        && p.abs_diff(after) <= 2
                        && *p != after
                        && *p != before
                        && (i16::from(after) - i16::from(*p)).signum() == direction
                })
            {
                out[i].pitch = p;
                out[i]
                    .provenance
                    .detail
                    .push_str(" · weak-beat passing tone");
            }
        }
    }
    if matches!(rules.species, Species::Fourth | Species::Fifth) {
        let mut removed = Vec::new();
        for i in 0..out.len().saturating_sub(2) {
            let prepared = &out[i];
            let crossed = &out[i + 1];
            let resolution = &out[i + 2];
            let boundary = leader.iter().any(|lead| lead.start == crossed.start);
            if !boundary
                || prepared.end() != crossed.start
                || c.overrides
                    .iter()
                    .any(|o| o.id == prepared.id || o.id == crossed.id)
            {
                continue;
            }
            let Some(upper) = leader.iter().find(|lead| lead.start == crossed.start) else {
                continue;
            };
            let consonant = theory::is_consonant(prepared.pitch, upper.pitch);
            if consonant
                || prepared.pitch > resolution.pitch && prepared.pitch - resolution.pitch <= 2
            {
                out[i].length += crossed.length;
                out[i].provenance.detail.push_str(if consonant {
                    " · consonant syncopation"
                } else {
                    " · prepared suspension resolving downward"
                });
                removed.push(i + 1);
            }
        }
        for i in removed.into_iter().rev() {
            out.remove(i);
        }
        if rules.species == Species::Fourth
            && out.len() > 1
            && !c.overrides.iter().any(|o| o.id == out[0].id)
        {
            out.remove(0);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canon_is_an_exact_interval_and_delay() {
        let mut c = Composition::default();
        c.voices[2].enabled = true;
        let leader = super::super::render(&c).unwrap();
        c.voices[4].low = 24;
        c.voices[4].high = 96;
        c.voices[4].counter.species = Species::Canon;
        let (follower, _) = compose(&c, &leader.notes).unwrap();
        for n in follower {
            let lead = leader
                .notes
                .iter()
                .find(|p| p.voice == Voice::Melody && p.start + 48 == n.start)
                .unwrap();
            assert_eq!(i16::from(n.pitch), i16::from(lead.pitch) - 12);
        }
    }
    #[test]
    fn no_leader_is_an_explicit_refusal() {
        let c = Composition::default();
        let mut c = c;
        c.voices[4].counter.species = Species::First;
        assert!(compose(&c, &[]).is_err());
    }
}

/// Revalidate the final notes, including locked edits and ties, at every attack
/// in either part. Only the selected counterpoint profile restricts these voices.
pub fn validate(
    c: &Composition,
    events: &[NoteEvent],
    findings: &mut Vec<Finding>,
) -> Result<(), String> {
    let rules = &c.voices[4].counter;
    if !c.voices[4].enabled || rules.species == Species::Off || rules.species == Species::Canon {
        return Ok(());
    }
    if rules.species == Species::Canon {
        let before = findings.len();
        for leader in events.iter().filter(|n| n.voice == rules.leader) {
            let id = identity(&[4, leader.id, 0x63616e6f6e]);
            let valid = events.iter().find(|n| n.id == id).is_some_and(|n| {
                n.voice == Voice::Counterpoint
                    && n.start == leader.start + rules.delay
                    && n.length == leader.length
                    && i32::from(n.pitch) == i32::from(leader.pitch) + i32::from(rules.interval)
            });
            if !valid {
                findings.push(Finding{rule:"counterpoint.canon".into(),events:vec![leader.id,id],detail:format!("Literal canon no longer matches its leader at tick {}; adjust the conflicting lock or choose another rule set",leader.start)});
            }
        }
        if rules.strict && findings.len() > before {
            return Err(findings[before].detail.clone());
        }
        return Ok(());
    }
    let mut upper = events
        .iter()
        .filter(|n| n.voice == rules.leader)
        .collect::<Vec<_>>();
    upper.sort_by_key(|n| n.start);
    let mut lower = events
        .iter()
        .filter(|n| n.voice == Voice::Counterpoint)
        .collect::<Vec<_>>();
    lower.sort_by_key(|n| n.start);
    let mut times = upper
        .iter()
        .chain(&lower)
        .map(|n| n.start)
        .collect::<Vec<_>>();
    times.sort();
    times.dedup();
    let mut previous = None;
    let begin = findings.len();
    for tick in times {
        let Some(a) = lower.iter().find(|n| n.start <= tick && n.end() > tick) else {
            continue;
        };
        let Some(b) = upper.iter().find(|n| n.start <= tick && n.end() > tick) else {
            continue;
        };
        let mut report = |rule: &str, detail: &str| {
            findings.push(Finding {
                rule: format!("counterpoint.{rule}"),
                events: vec![a.id, b.id],
                detail: format!("{} at tick {tick}: {detail}", rules.species.label()),
            })
        };
        if a.pitch >= b.pitch {
            report("crossing", "the follower must remain below its leader");
        }
        if !theory::is_consonant(a.pitch, b.pitch) {
            let at = lower.iter().position(|n| n.id == a.id).unwrap_or(0);
            let next = lower.get(at + 1);
            let before = at.checked_sub(1).map(|i| lower[i]);
            let suspension = matches!(rules.species, Species::Fourth | Species::Fifth)
                && a.start < tick
                && next.is_some_and(|n| {
                    a.pitch > n.pitch
                        && a.pitch - n.pitch <= 2
                        && n.start == a.end()
                        && theory::is_consonant(n.pitch, b.pitch)
                });
            let passing = matches!(
                rules.species,
                Species::Second | Species::Third | Species::Fifth
            ) && a.start > b.start
                && before.zip(next).is_some_and(|(p, n)| {
                    let left = i16::from(a.pitch) - i16::from(p.pitch);
                    let right = i16::from(n.pitch) - i16::from(a.pitch);
                    left != 0
                        && right != 0
                        && left.abs() <= 2
                        && right.abs() <= 2
                        && left.signum() == right.signum()
                });
            if !suspension && !passing {
                report(
                    "dissonance",
                    "dissonance lacks a prepared suspension or stepwise weak-beat path",
                );
            }
        }
        if let Some((low, high)) = previous {
            if theory::is_parallel_perfect((low, high), (a.pitch, b.pitch)) {
                report("parallel", "parallel perfect intervals");
            }
            let dl = i16::from(a.pitch) - i16::from(low);
            let dh = i16::from(b.pitch) - i16::from(high);
            if dl != 0
                && dl.signum() == dh.signum()
                && dh.abs() > 2
                && [0, 7].contains(&(b.pitch.abs_diff(a.pitch) % 12))
            {
                report(
                    "direct-perfect",
                    "similar motion with an upper-voice leap into a perfect interval",
                );
            }
        }
        if let Some(interval) = rules.invertible {
            let inverted = u16::from(a.pitch) + u16::from(interval);
            if inverted > 127 || !theory::is_consonant(inverted as u8, b.pitch) {
                report("inversion", "the selected inversion creates a dissonance");
            }
        }
        previous = Some((a.pitch, b.pitch));
    }
    if rules.strict && findings.len() > begin {
        return Err(findings[begin].detail.clone());
    }
    Ok(())
}
