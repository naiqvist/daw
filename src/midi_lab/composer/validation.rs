use super::*;
use std::collections::BTreeSet;

fn curve_valid(points: &[(u16, i16)]) -> bool {
    points.len() <= 128
        && points
            .iter()
            .all(|(x, y)| *x <= 1000 && (0..=100).contains(y))
}

pub(super) fn document(c: &Composition) -> Result<(), String> {
    if c.tension_map.register > 2
        || c.tension_map.velocity > 40
        || c.tension_map.density > 8
        || c.tension_map.gate > 100
    {
        return Err("Tension mapping exceeds supported bounds".into());
    }
    let mut ids = BTreeSet::new();
    for id in c
        .harmony
        .iter()
        .map(|h| h.id)
        .chain(c.motifs.iter().map(|m| m.id))
        .chain(c.placements.iter().map(|p| p.id))
        .chain(c.sections.iter().map(|s| s.id))
        .chain(c.modulations.iter().map(|m| m.id))
    {
        if !ids.insert(id) {
            return Err("Authored objects must have distinct stable IDs".into());
        }
    }
    if c.next_id == u64::MAX
        || c.event_destinations.len() > MAX_EVENTS
        || c.modulations.len() > 1024
        || c.comparisons.len() > 32
        || !curve_valid(&c.tension)
    {
        return Err("Document exceeds identity, curve or comparison limits".into());
    }
    for h in &c.harmony {
        let members = h
            .material
            .members
            .iter()
            .map(|m| m.id)
            .collect::<BTreeSet<_>>();
        if h.voicing.center > 127
            || h.voicing.doubled.len() > 128
            || h.voicing.omitted.len() > 128
            || h.voicing.offsets.len() > 128
            || h.voicing
                .omitted
                .iter()
                .chain(&h.voicing.doubled)
                .chain(h.voicing.offsets.keys())
                .any(|id| !members.contains(id))
        {
            return Err("Voicing refers to missing members or exceeds its bounds".into());
        }
    }
    for v in &c.voices {
        let r = &v.rhythm;
        if r.custom_length == 0
            || r.custom_length > MAX_TICKS
            || r.offsets.len() > 2048
            || r.accents.len() > 2048
            || v.bass.kick_length > MAX_TICKS
            || v.bass.kick.iter().any(|p| *p >= v.bass.kick_length)
            || v.bass.tuning.len() > 16
            || v.bass.tuning.iter().any(|p| *p > 127)
            || !curve_valid(&v.melody.curve)
            || !curve_valid(&v.velocity_curve)
            || !curve_valid(&v.gate_curve)
            || v.counter.delay > MAX_TICKS
            || v.counter
                .invertible
                .is_some_and(|i| ![12, 16, 19].contains(&i))
        {
            return Err("Voice cell, curve or instrument setting exceeds its bounds".into());
        }
        let mut ids = BTreeSet::new();
        for p in &r.custom {
            if !ids.insert(p.id)
                || p.length == 0
                || p.velocity == 0
                || p.velocity > 127
                || p.start
                    .checked_add(p.length)
                    .is_none_or(|n| n > r.custom_length)
            {
                return Err("Written rhythm has duplicate identities or invalid events".into());
            }
        }
    }
    let mut count = 0;
    for m in &c.motifs {
        count += m.notes.len();
        if m.length == 0 || m.length > MAX_TICKS || count > MAX_EVENTS {
            return Err("Motif bank exceeds time or event limits".into());
        }
        let mut ids = BTreeSet::new();
        for n in &m.notes {
            if !ids.insert(n.id)
                || n.length == 0
                || n.velocity == 0
                || n.velocity > 127
                || n.start
                    .checked_add(n.length)
                    .is_none_or(|end| end > m.length)
                || m.frame == PitchFrame::Absolute && !(0..=127).contains(&n.pitch)
            {
                return Err("Motif contains an invalid source note".into());
            }
        }
    }
    for p in &c.placements {
        if p.anchor > 127
            || p.start >= c.length
            || p.transforms.len() > 32
            || !c.motifs.iter().any(|m| m.id == p.motif)
        {
            return Err("Placement has an invalid anchor, source or transformation count".into());
        }
    }
    let mut spans = Vec::new();
    for p in &c.placements {
        let m = c
            .motifs
            .iter()
            .find(|m| m.id == p.motif)
            .ok_or("Missing placement source")?;
        let length = super::motif::transform(m, &p.transforms, c.key_at(p.start))?.length;
        let end = p
            .start
            .checked_add(length)
            .filter(|end| *end <= c.length)
            .ok_or("A placement exceeds the composition")?;
        spans.push((p.voice.index(), p.start, end));
    }
    spans.sort();
    if spans
        .windows(2)
        .any(|pair| pair[0].0 == pair[1].0 && pair[0].2 > pair[1].1)
    {
        return Err(
            "Motif placements overlap within a voice; move or fragment one placement explicitly"
                .into(),
        );
    }
    let mut ids = BTreeSet::new();
    for o in &c.overrides {
        if !ids.insert(o.id)
            || o.pitch.is_some_and(|p| p > 127)
            || o.velocity.is_some_and(|p| p == 0 || p > 127)
            || o.length == Some(0)
            || o.inserted.as_ref().is_some_and(|n| n.id != o.id)
        {
            return Err("A note override contains invalid values or duplicate identities".into());
        }
    }
    let mut times = BTreeSet::new();
    for m in &c.modulations {
        if !times.insert(m.start) || m.pitch.is_some_and(|p| p > 127) {
            return Err("Modulation positions must be distinct and pitches valid".into());
        }
    }
    for s in &c.sections {
        if s.length == 0 || s.start.checked_add(s.length).is_none_or(|n| n > c.length) {
            return Err("Section exceeds its source timeline".into());
        }
    }
    if c.snapshot
        .iter()
        .chain(&c.comparisons)
        .map(|s| s.events.len())
        .sum::<usize>()
        > MAX_EVENTS * 4
    {
        return Err("Saved event banks exceed the combined 131072-event budget; remove a comparison before saving another".into());
    }
    for snapshot in c.snapshot.iter().chain(c.comparisons.iter()) {
        if snapshot.harmony.len() > MAX_TICKS as usize
            || snapshot.voicings.len() > MAX_TICKS as usize
        {
            return Err("Saved analysis exceeds its resource budget".into());
        }
        let length = if snapshot.length == 0 {
            c.length
        } else {
            snapshot.length
        };
        if length > MAX_TICKS
            || snapshot.events.len() > MAX_EVENTS
            || snapshot.input.len() > 8 * 1024 * 1024
        {
            return Err("Snapshot exceeds resource limits".into());
        }
        let mut ids = BTreeSet::new();
        for n in &snapshot.events {
            if !ids.insert(n.id)
                || n.pitch > 127
                || n.velocity == 0
                || n.velocity > 127
                || n.length == 0
                || n.start.checked_add(n.length).is_none_or(|end| end > length)
            {
                return Err("Saved snapshot contains invalid MIDI events".into());
            }
        }
    }
    Ok(())
}
