use super::*;
use crate::{midi_lab::Voice, theory::harmony::Layout};

const CANDIDATES: usize = 32;
const TRANSITIONS: usize = 262144;

pub fn cost(
    previous: &[(u64, u8)],
    notes: &[(u64, u8)],
    weights: &Weights,
    bass: Option<u8>,
) -> Cost {
    let mut result = Cost::default();
    for (i, (_, p)) in notes.iter().enumerate() {
        if let Some((_, before)) = previous.get(i) {
            let movement = (i64::from(*p) - i64::from(*before)).abs();
            result.motion += movement * i64::from(weights.motion);
            result.leap = result.leap.max(movement * i64::from(weights.leap));
        }
        if previous.iter().any(|(_, before)| before == p) {
            result.common -= i64::from(weights.common);
        }
        if let Some(bass) = bass {
            if *p <= bass {
                result.crossing +=
                    i64::from(weights.crossing) * i64::from(bass.saturating_sub(*p) + 1);
            }
            if p % 12 == bass % 12 {
                result.doubling += i64::from(weights.doubling);
            }
        }
        if let Some((_, next)) = notes.get(i + 1) {
            let interval = next.saturating_sub(*p);
            if *p < 48 && interval < 5 {
                result.spacing += i64::from(5 - interval) * i64::from(weights.spacing);
            }
            result.spacing += i64::from(interval.saturating_sub(12)) * i64::from(weights.spacing);
        }
    }
    for a in 0..notes.len().min(previous.len()) {
        for b in a + 1..notes.len().min(previous.len()) {
            if crate::theory::is_parallel_perfect(
                (previous[a].1, previous[b].1),
                (notes[a].1, notes[b].1),
            ) {
                result.parallel += i64::from(weights.parallel);
            }
        }
    }
    result
}

pub fn candidates(c: &Composition, h: &HarmonySpan) -> Result<Vec<Vec<(u64, u8)>>, String> {
    let range = &c.voices[Voice::Chords.index()];
    let v = &h.voicing;
    let mut material = h.material.clone();
    material.members.retain(|m| !v.omitted.contains(&m.id));
    match v.layout {
        Layout::Rootless => {
            let root = material
                .root
                .ok_or("Rootless voicing requires a selected harmonic root")?;
            material.members.retain(|m| m.pc != root);
        }
        Layout::Shell => {
            if material.root.is_none() {
                return Err("Shell voicing requires a harmonic reading".into());
            }
            material
                .members
                .retain(|m| matches!(m.degree, Some(1 | 3 | 7)));
        }
        _ => {}
    }
    let exact = material.members.iter().any(|m| m.pitch.is_some())
        || matches!(material.bass, Some(crate::theory::material::Bass::Pitch(_)));
    let original = material.realise(range.low, range.high, v.center)?;
    if original.is_empty() {
        return Ok(vec![vec![]]);
    }
    let mut initial = original
        .iter()
        .map(|(id, p)| (*id, i16::from(*p)))
        .collect::<Vec<_>>();
    for _ in 0..usize::from(v.inversion) {
        let mut first = initial.remove(0);
        let top = initial.last().map_or(first.1, |x| x.1);
        first.1 += 12 * ((top - first.1) / 12 + 1);
        initial.push(first);
    }
    match v.layout {
        Layout::Open => {
            for (i, (_, p)) in initial.iter_mut().enumerate() {
                if i % 2 == 1 {
                    *p += 12;
                }
            }
        }
        Layout::Drop2 | Layout::Drop3 | Layout::Drop24 => {
            let drops: &[usize] = match v.layout {
                Layout::Drop2 => &[2],
                Layout::Drop3 => &[3],
                _ => &[2, 4],
            };
            for &drop in drops {
                let at = initial
                    .len()
                    .checked_sub(drop)
                    .ok_or("Drop layout needs more members")?;
                initial[at].1 -= 12;
            }
        }
        Layout::Upper => {
            for (id, p) in &mut initial {
                if material
                    .members
                    .iter()
                    .find(|m| m.id == *id)
                    .is_some_and(|m| {
                        if material.components.len() > 1 {
                            m.component == Some(0)
                        } else {
                            m.degree.is_some_and(|d| d >= 9)
                        }
                    })
                {
                    *p += 12;
                }
            }
        }
        Layout::Quartal => {
            // A finite DFS over actual members; no invented pitch classes.
            let mut found = None;
            let mut stack = Vec::new();
            let mut used = vec![false; material.members.len()];
            let mut budget = 8192usize;
            fn walk(
                m: &[crate::theory::material::Member],
                low: u8,
                high: u8,
                stack: &mut Vec<(u64, u8)>,
                used: &mut [bool],
                budget: &mut usize,
            ) -> bool {
                if stack.len() == m.len() {
                    return true;
                }
                if *budget == 0 {
                    return false;
                }
                *budget -= 1;
                for i in 0..m.len() {
                    if used[i] {
                        continue;
                    }
                    for pitch in low..=high {
                        if pitch % 12 != m[i].pc
                            || m[i].pitch.is_some_and(|p| p != pitch)
                            || stack.last().is_some_and(|(_, p)| {
                                pitch <= *p || ![5, 6].contains(&(pitch - *p))
                            })
                        {
                            continue;
                        }
                        used[i] = true;
                        stack.push((m[i].id, pitch));
                        if walk(m, low, high, stack, used, budget) {
                            return true;
                        }
                        stack.pop();
                        used[i] = false;
                    }
                }
                false
            }
            if material.bass.is_some() {
                return Err(
                    "Quartal layout with a separate slash bass requires an explicit note voicing"
                        .into(),
                );
            }
            if walk(
                &material.members,
                range.low,
                range.high,
                &mut stack,
                &mut used,
                &mut budget,
            ) {
                found = Some(stack);
            }
            initial = found
                .ok_or(if budget == 0 {
                    "Quartal search exhausted its 8192-expansion budget"
                } else {
                    "No quartal spacing fits these exact members and range"
                })?
                .into_iter()
                .map(|(id, p)| (id, i16::from(p)))
                .collect();
        }
        _ => {}
    }
    for (id, p) in &mut initial {
        if let Some(offset) = v.offsets.get(id) {
            *p = p
                .checked_add(
                    offset
                        .checked_mul(12)
                        .ok_or("Octave displacement overflow")?,
                )
                .ok_or("Pitch overflow")?;
        }
    }
    for id in &v.doubled {
        let (_, p) = initial
            .iter()
            .find(|(member, _)| member == id)
            .ok_or("Doubling refers to an absent member")?;
        initial.push((identity(&[*id, 0x646f75626c65]), p + 12));
    }
    initial.sort_by_key(|(id, p)| (*p, *id));
    let can_rotate = v.lead
        && !exact
        && v.inversion == 0
        && material.bass.is_none()
        && matches!(v.layout, Layout::Closed | Layout::Rootless | Layout::Shell)
        && v.offsets.is_empty()
        && v.doubled.is_empty();
    let rotations = if can_rotate { initial.len().min(12) } else { 1 };
    let shifts: &[i16] = if v.lead && !exact {
        &[0, -12, 12, -24, 24]
    } else {
        &[0]
    };
    let mut result = Vec::new();
    for _ in 0..rotations {
        for shift in shifts {
            if initial.iter().any(|(_, p)| {
                *p + shift < i16::from(range.low) || *p + shift > i16::from(range.high)
            }) {
                continue;
            }
            result.push(
                initial
                    .iter()
                    .map(|(id, p)| (*id, (*p + shift) as u8))
                    .collect::<Vec<_>>(),
            );
        }
        let mut first = initial.remove(0);
        let ceiling = initial.last().map_or(first.1, |x| x.1);
        first.1 += 12 * ((ceiling - first.1) / 12 + 1);
        initial.push(first);
    }
    result.sort();
    result.dedup();
    if result.is_empty() {
        return Err(
            "Voicing cannot fit without changing explicit notes or the requested spacing".into(),
        );
    }
    result.sort_by_key(|notes| {
        let register: i64 = notes
            .iter()
            .map(|(_, p)| (i64::from(*p) - i64::from(v.center)).abs())
            .sum();
        (register, notes.clone())
    });
    result.truncate(CANDIDATES);
    result.sort();
    Ok(result)
}

pub fn solve(c: &Composition, outer: &[NoteEvent]) -> Result<Vec<VoicingDecision>, String> {
    let mut spans = c.harmony.iter().collect::<Vec<_>>();
    spans.sort_by_key(|h| (h.start, h.id));
    let mut chosen: Vec<VoicingDecision> = Vec::new();
    let mut previous = Vec::new();
    let mut searched = 0;
    // Receding two-chord horizon bounds work independently of song length.
    // The UI reports this candidate horizon, not a globally optimal chorale.
    for (at, h) in spans.iter().enumerate() {
        let mut options = candidates(c, h)?;
        // Locked chord notes participate in the cost at their first attack.
        // Manual chromatic pitches remain explicit, even outside the collection.
        if let Some(pulse) = super::rhythm::pulses(c, Voice::Chords)?
            .iter()
            .find(|p| c.harmony_at(p.start).is_some_and(|span| span.id == h.id))
        {
            for notes in &mut options {
                for (member, pitch) in notes {
                    let id = identity(&[0, h.id, pulse.id, *member]);
                    if let Some(edit) = c.overrides.iter().find(|o| o.id == id && !o.deleted) {
                        if let Some(p) = edit.pitch {
                            *pitch = p;
                        }
                    }
                }
            }
        }
        for notes in &mut options {
            notes.sort_by_key(|(id, p)| (*p, *id));
        }
        let next = if let Some(next) = spans.get(at + 1) {
            candidates(c, next)?
        } else if c.looping && !chosen.is_empty() {
            vec![chosen[0].notes.clone()]
        } else {
            vec![vec![]]
        };
        let bass = outer
            .iter()
            .filter(|n| n.voice == Voice::Bass && n.start <= h.start && n.end() > h.start)
            .map(|n| n.pitch)
            .min();
        let soprano = outer
            .iter()
            .filter(|n| n.voice == Voice::Melody && n.start <= h.start && n.end() > h.start)
            .map(|n| n.pitch)
            .max();
        let mut ranked = Vec::new();
        for notes in options {
            let mut own = cost(&previous, &notes, &c.weights, bass);
            if let Some(top) = soprano {
                own.crossing += notes
                    .iter()
                    .filter(|(_, p)| *p > top)
                    .map(|(_, p)| i64::from(*p - top) * i64::from(c.weights.crossing))
                    .sum::<i64>();
            }
            let mut future = None;
            for after in &next {
                searched += 1;
                if searched > TRANSITIONS {
                    return Err("Voicing search exceeded 262144 transitions; reduce the passage or candidate scope".into());
                }
                let n = cost(&notes, after, &c.weights, None).total();
                future = Some(future.map_or(n, |old: i64| old.min(n)));
            }
            let center = notes
                .iter()
                .map(|(_, p)| (i64::from(*p) - i64::from(h.voicing.center)).abs())
                .sum::<i64>();
            ranked.push((
                own.total() + future.unwrap_or(0) + if previous.is_empty() { center } else { 0 },
                notes,
                own,
            ));
        }
        ranked.sort_by_key(|(score, notes, _)| (*score, notes.clone()));
        let count = ranked.len();
        let winner = ranked
            .first()
            .ok_or("No candidate satisfies fixed chord notes")?;
        let runner_up = ranked
            .get(1)
            .map(|(score, notes, _)| (notes.iter().map(|(_, p)| *p).collect(), *score));
        previous = winner.1.clone();
        chosen.push(VoicingDecision {
            harmony: h.id,
            notes: previous.clone(),
            cost: winner.2.clone(),
            runner_up,
            candidates: count,
            searched,
        });
    }
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_chord_motion_search_matches_exhaustive_candidate_pairs() {
        let mut c = Composition::default();
        c.harmony.truncate(2);
        c.looping = false;
        c.weights = Weights {
            motion: 1,
            leap: 0,
            common: 0,
            spacing: 0,
            parallel: 0,
            crossing: 0,
            doubling: 0,
        };
        let first = candidates(&c, &c.harmony[0]).unwrap();
        let second = candidates(&c, &c.harmony[1]).unwrap();
        let mut reference = Vec::new();
        for a in &first {
            for b in &second {
                // Independent reference arithmetic: ordered voice motion plus
                // the documented initial register preference, then lexical ties.
                let motion = a
                    .iter()
                    .zip(b)
                    .map(|(x, y)| (i64::from(x.1) - i64::from(y.1)).abs())
                    .sum::<i64>();
                let center = a
                    .iter()
                    .map(|x| (i64::from(x.1) - i64::from(c.harmony[0].voicing.center)).abs())
                    .sum::<i64>();
                reference.push((motion + center, a.clone(), b.clone()));
            }
        }
        reference.sort();
        let answer = solve(&c, &[]).unwrap();
        assert_eq!(answer[0].notes, reference[0].1);
        assert_eq!(answer[1].notes, reference[0].2);
    }
    #[test]
    fn exact_voicings_do_not_follow_register_preferences() {
        let mut c = Composition::default();
        c.harmony.truncate(1);
        c.harmony[0].material = crate::theory::material::Material::parse("notes:C4,E5,G4").unwrap();
        let a = solve(&c, &[]).unwrap();
        c.harmony[0].voicing.center = 48;
        let b = solve(&c, &[]).unwrap();
        assert_eq!(a[0].notes, b[0].notes);
    }
    #[test]
    fn drops_are_actual_interval_changes() {
        let mut c = Composition::default();
        c.harmony.truncate(1);
        c.harmony[0].voicing.lead = false;
        let base = candidates(&c, &c.harmony[0]).unwrap()[0].clone();
        c.harmony[0].voicing.layout = Layout::Drop2;
        c.voices[0].low = 24;
        let drop = candidates(&c, &c.harmony[0]).unwrap()[0].clone();
        let id = base[base.len() - 2].0;
        assert_eq!(
            drop.iter().find(|n| n.0 == id).unwrap().1 + 12,
            base[base.len() - 2].1
        );
    }
    #[test]
    fn explicit_tiebreak_is_repeatable() {
        let c = Composition::default();
        assert_eq!(solve(&c, &[]).unwrap(), solve(&c, &[]).unwrap());
    }
}
