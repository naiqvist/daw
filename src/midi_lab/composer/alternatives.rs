use super::*;
use crate::midi_lab::Voice;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub recipe: Composition,
    pub notes: Vec<NoteEvent>,
    pub differences: Vec<String>,
}

pub fn describe(c: &Composition, voice: Voice) -> String {
    let v = &c.voices[voice.index()];
    if voice == Voice::Bass {
        format!(
            "{} · {} · {} · approach {}",
            v.bass.role.label(),
            v.rhythm.kind.label(),
            v.bass.groove.label(),
            v.bass.approaches.label()
        )
    } else {
        format!(
            "{} · {} · {} · {}",
            v.melody.contour.label(),
            v.rhythm.kind.label(),
            v.melody.movement.label(),
            v.melody.development.label()
        )
    }
}
fn sounding(notes: &[NoteEvent], voice: Voice) -> Vec<(u8, u32, u32, u8)> {
    notes
        .iter()
        .filter(|n| n.voice == voice)
        .map(|n| (n.pitch, n.start, n.length, n.velocity))
        .collect()
}
fn distance(a: &[NoteEvent], b: &[NoteEvent], voice: Voice) -> u64 {
    let a = sounding(a, voice);
    let b = sounding(b, voice);
    let mut cost = a.len().abs_diff(b.len()) as u64 * 32;
    for (a, b) in a.iter().zip(&b) {
        cost += u64::from(a.0.abs_diff(b.0))
            + u64::from(a.1.abs_diff(b.1)) / 6
            + u64::from(a.2.abs_diff(b.2)) / 6;
    }
    cost
}

pub fn explore(
    source: &Composition,
    voice: Voice,
    action: VariationAction,
    limit: usize,
) -> Result<Vec<Candidate>, String> {
    if !source.voices[voice.index()].enabled {
        return Err("Enable the selected voice before exploring alternatives".into());
    }
    let original = render(source)?;
    let original_notes = original
        .notes
        .iter()
        .filter(|n| n.voice == voice)
        .cloned()
        .collect::<Vec<_>>();
    if original_notes.is_empty() {
        return Err("The selected voice has no notes to vary".into());
    }
    let mut options = Vec::new();
    let limit = limit.clamp(1, 12);
    'candidate: for ordinal in 1..=48u16 {
        let mut c = source.clone();
        c.frozen = false;
        c.snapshot = None;
        let v = &mut c.voices[voice.index()];
        match action {
            VariationAction::NewPitches => {
                if voice == Voice::Bass {
                    v.bass.variation = v.bass.variation.wrapping_add(ordinal);
                    v.bass.role = BassRole::ALL[usize::from(ordinal) % BassRole::ALL.len()];
                } else {
                    v.melody.variation = v.melody.variation.wrapping_add(ordinal);
                    v.melody.contour = Contour::ALL[usize::from(ordinal) % Contour::ALL.len()];
                    v.melody.movement =
                        Movement::ALL[usize::from(ordinal / 7) % Movement::ALL.len()];
                }
            }
            VariationAction::NewRhythm => {
                v.rhythm.rotation = v.rhythm.rotation.wrapping_add(ordinal);
                if v.rhythm.kind == RhythmKind::Hold {
                    v.rhythm.kind = RhythmKind::Quarter;
                }
                v.rhythm.offsets = vec![i16::try_from(ordinal % 12).unwrap_or(0)];
            }
            VariationAction::Answer => {
                v.melody.development =
                    Development::ALL[usize::from(ordinal) % Development::ALL.len()];
                v.melody.variation = v.melody.variation.wrapping_add(ordinal);
                v.bass.variation = v.bass.variation.wrapping_add(ordinal);
            }
            VariationAction::Approach => {
                let choices = [
                    Decoration::None,
                    Decoration::Passing,
                    Decoration::Chromatic,
                    Decoration::Anticipation,
                ];
                v.melody.decoration = choices[usize::from(ordinal) % choices.len()];
                v.bass.approaches = v.melody.decoration;
                v.bass.variation = v.bass.variation.wrapping_add(ordinal);
            }
            VariationAction::Simplify => {
                v.rhythm.kind = if ordinal % 2 == 0 {
                    RhythmKind::Quarter
                } else {
                    RhythmKind::Hold
                };
                v.melody.movement = Movement::Repeated;
                v.melody.decoration = Decoration::None;
                v.bass.role = BassRole::Foundation;
                v.bass.fill_every = 0;
            }
            VariationAction::Range => {
                v.low = v.low.saturating_sub((ordinal % 4) as u8 * 3);
                v.high = v.high.saturating_add((ordinal % 4) as u8 * 3).min(127);
                v.melody.contour = Contour::ALL[usize::from(ordinal) % Contour::ALL.len()];
            }
            VariationAction::Develop => {
                v.melody.development =
                    Development::ALL[usize::from(ordinal) % Development::ALL.len()];
                v.melody.interval = (ordinal % 7) as i16 - 3;
            }
            VariationAction::Fill => {
                v.bass.variation = v.bass.variation.wrapping_add(ordinal);
                v.bass.fill_every = 2 + (ordinal % 3) as u8;
                v.bass.fill_beats = 1 + (ordinal % 2) as u8;
            }
            VariationAction::Space => {
                v.bass.leave_melody_space = true;
                v.bass.fill_every = 0;
                v.rhythm.kind = if ordinal % 2 == 0 {
                    RhythmKind::Hold
                } else {
                    RhythmKind::Quarter
                };
                v.rhythm.gate = 40 + (ordinal % 5) as u8 * 10;
            }
        }
        for p in c.placements.iter_mut().filter(|p| p.voice == voice) {
            match action {
                VariationAction::NewPitches | VariationAction::Answer => {
                    let frame = c
                        .motifs
                        .iter()
                        .find(|m| m.id == p.motif)
                        .map(|m| m.frame)
                        .unwrap_or(PitchFrame::Absolute);
                    let by = (ordinal % 13) as i16 - 6;
                    p.transforms.push(match frame {
                        PitchFrame::Absolute | PitchFrame::Chromatic => Transform::Transpose(by),
                        PitchFrame::Diatonic => Transform::Diatonic(by),
                        PitchFrame::ChordRoles => Transform::Degree(by),
                    });
                }
                VariationAction::NewRhythm => p
                    .transforms
                    .push(Transform::Rotate(u32::from(ordinal % 12))),
                VariationAction::Develop => p.transforms.push(if ordinal % 2 == 0 {
                    Transform::Retrograde
                } else {
                    Transform::Transpose((ordinal % 9) as i16 - 4)
                }),
                _ => {}
            }
        }
        if action == VariationAction::Answer {
            let opening = source.meter.bar()?;
            for note in original_notes.iter().filter(|n| n.start < opening) {
                if !c.overrides.iter().any(|o| o.id == note.id) {
                    c.pin(note);
                }
            }
        }
        if action == VariationAction::Approach {
            for note in original_notes
                .iter()
                .filter(|n| n.provenance.rule.contains("arrival"))
            {
                if !c.overrides.iter().any(|o| o.id == note.id) {
                    c.pin(note);
                }
            }
        }
        let Ok(mut rendered) = render(&c) else {
            continue;
        };
        if action == VariationAction::NewPitches {
            let next = sounding(&rendered.notes, voice);
            let old = sounding(&original.notes, voice);
            if next.iter().map(|n| (n.1, n.2)).collect::<Vec<_>>()
                != old.iter().map(|n| (n.1, n.2)).collect::<Vec<_>>()
            {
                continue;
            }
        }
        if action == VariationAction::NewRhythm {
            let next = rendered
                .notes
                .iter()
                .filter(|n| n.voice == voice)
                .cloned()
                .collect::<Vec<_>>();
            if next.len() != original_notes.len() {
                continue;
            }
            for (note, original) in next.iter().zip(&original_notes) {
                if c.overrides
                    .iter()
                    .any(|o| o.id == note.id && o.pitch.is_some_and(|p| p != original.pitch))
                {
                    continue 'candidate;
                }
                let mut edit = c
                    .overrides
                    .iter()
                    .find(|o| o.id == note.id)
                    .cloned()
                    .unwrap_or(Override {
                        id: note.id,
                        ..Override::default()
                    });
                edit.pitch = Some(original.pitch);
                c.overrides.retain(|o| o.id != note.id);
                c.overrides.push(edit);
            }
            let Ok(next) = render(&c) else {
                continue;
            };
            rendered = next;
            if sounding(&rendered.notes, voice)
                .iter()
                .map(|n| n.0)
                .collect::<Vec<_>>()
                != original_notes.iter().map(|n| n.pitch).collect::<Vec<_>>()
            {
                continue;
            }
        }
        for lock in &source.overrides {
            if let Some(before) = original.notes.iter().find(|n| n.id == lock.id) {
                let Some(after) = rendered.notes.iter().find(|n| n.id == lock.id) else {
                    continue 'candidate;
                };
                if lock.pitch.is_some() && before.pitch != after.pitch
                    || lock.start.is_some() && before.start != after.start
                    || lock.length.is_some() && before.length != after.length
                    || lock.velocity.is_some() && before.velocity != after.velocity
                {
                    continue 'candidate;
                }
            }
        }
        let signature = sounding(&rendered.notes, voice);
        if signature == sounding(&original.notes, voice)
            || options
                .iter()
                .any(|candidate: &Candidate| sounding(&candidate.notes, voice) == signature)
        {
            continue;
        }
        let differences = diff(source, &c);
        options.push(Candidate {
            name: describe(&c, voice),
            recipe: c,
            notes: rendered.notes,
            differences,
        });
    }
    if options.is_empty() {
        return Err(
            "No distinct alternative satisfies the current locks and requested invariants".into(),
        );
    }
    // Deterministic farthest-first selection exposes actual audible dimensions.
    let mut chosen = Vec::new();
    while chosen.len() < limit && !options.is_empty() {
        let best = (0..options.len())
            .max_by_key(|i| {
                let nearest = chosen
                    .iter()
                    .map(|candidate: &Candidate| {
                        distance(&options[*i].notes, &candidate.notes, voice)
                    })
                    .min()
                    .unwrap_or_else(|| distance(&original.notes, &options[*i].notes, voice));
                (nearest, std::cmp::Reverse(options[*i].name.clone()))
            })
            .unwrap_or(0);
        chosen.push(options.remove(best));
    }
    Ok(chosen)
}

pub fn diff(a: &Composition, b: &Composition) -> Vec<String> {
    let mut result = Vec::new();
    if a.harmony != b.harmony {
        result.push("Harmony or voicing changed".into());
    }
    if a.key != b.key {
        result.push("Tonal context changed".into());
    }
    for voice in Voice::ALL {
        let av = &a.voices[voice.index()];
        let bv = &b.voices[voice.index()];
        if av.rhythm != bv.rhythm {
            result.push(format!(
                "{} rhythm: {}",
                voice.label(),
                bv.rhythm.kind.label()
            ));
        }
        if av.melody != bv.melody {
            result.push(format!("{}: {}", voice.label(), describe(b, voice)));
        }
        if av.bass != bv.bass && voice == Voice::Bass {
            result.push(format!("Bass: {}", describe(b, voice)));
        }
        if av.low != bv.low || av.high != bv.high {
            result.push(format!("{} register {}–{}", voice.label(), bv.low, bv.high));
        }
        if av.enabled != bv.enabled {
            result.push(format!(
                "{} {}",
                voice.label(),
                if bv.enabled { "enabled" } else { "disabled" }
            ));
        }
    }
    if a.placements != b.placements {
        result.push("Motif development changed".into());
    }
    if a.form != b.form || a.sections != b.sections {
        result.push("Section arrangement changed".into());
    }
    result
}

pub fn save_snapshot(c: &mut Composition, label: String) -> Result<(), String> {
    let rendered = render(c)?;
    c.snapshot = Some(Snapshot {
        harmony: rendered.harmony.clone(),
        voicings: rendered.voicings.clone(),
        length: rendered.length,
        input: c.input()?,
        events: rendered.notes,
        label,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alternatives_are_distinct_reproducible_and_keep_rhythm() {
        let mut c = Composition::default();
        c.voices[0].enabled = false;
        c.voices[2].enabled = true;
        let before = render(&c).unwrap();
        let a = explore(&c, Voice::Melody, VariationAction::NewPitches, 4).unwrap();
        let b = explore(&c, Voice::Melody, VariationAction::NewPitches, 4).unwrap();
        assert_eq!(a, b);
        assert!(a.len() >= 3);
        for candidate in a {
            assert_eq!(
                candidate
                    .notes
                    .iter()
                    .map(|n| (n.start, n.length))
                    .collect::<Vec<_>>(),
                before
                    .notes
                    .iter()
                    .map(|n| (n.start, n.length))
                    .collect::<Vec<_>>()
            );
        }
    }
}
