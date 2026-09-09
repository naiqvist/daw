use super::*;
use crate::midi_lab::Voice;
use crate::theory::functional::Key;

fn checked_pitch(n: i32) -> Result<i16, String> {
    if !(0..=127).contains(&n) {
        Err("Motif transformation exceeds MIDI 0–127".into())
    } else {
        Ok(n as i16)
    }
}

pub fn transform(
    source: &Motif,
    operations: &[Transform],
    key: Option<&Key>,
) -> Result<Motif, String> {
    let mut motif = source.clone();
    if source.length == 0
        || source.length > MAX_TICKS
        || source.notes.len() > MAX_EVENTS
        || operations.len() > 32
        || source.notes.iter().any(|n| {
            n.length == 0
                || n.start
                    .checked_add(n.length)
                    .is_none_or(|end| end > source.length)
                || n.velocity == 0
                || n.velocity > 127
        })
    {
        return Err("Invalid motif length or event budget".into());
    }
    for op in operations {
        match *op {
            Transform::Transpose(by) => {
                if !matches!(motif.frame, PitchFrame::Absolute | PitchFrame::Chromatic) {
                    return Err("Semitone transpose needs absolute or chromatic material; select the corresponding degree transform for this frame".into());
                }
                for n in &mut motif.notes {
                    n.pitch = if motif.frame == PitchFrame::Absolute {
                        checked_pitch(i32::from(n.pitch) + i32::from(by))?
                    } else {
                        n.pitch.checked_add(by).ok_or("Interval overflow")?
                    };
                }
            }
            Transform::Diatonic(by) => {
                let key = key.ok_or("Diatonic transformation needs a key")?;
                if motif.frame == PitchFrame::Diatonic {
                    for n in &mut motif.notes {
                        n.pitch = n
                            .pitch
                            .checked_add(by)
                            .ok_or("Scale-step interval overflow")?;
                    }
                    continue;
                }
                if motif.frame != PitchFrame::Absolute {
                    return Err(
                        "Realise a relative motif before applying a diatonic pitch transform"
                            .into(),
                    );
                }
                for n in &mut motif.notes {
                    n.pitch = i16::from(key.step(
                        u8::try_from(n.pitch).map_err(|_| "Invalid motif pitch")?,
                        by,
                    )?);
                }
            }
            Transform::Degree(by) => {
                if motif.frame != PitchFrame::ChordRoles {
                    return Err("Degree transformation requires chord-role material".into());
                }
                for n in &mut motif.notes {
                    n.pitch = n.pitch.checked_add(by).ok_or("Degree overflow")?;
                }
            }
            Transform::Invert(axis) => {
                for n in &mut motif.notes {
                    let value = i32::from(axis) * 2 - i32::from(n.pitch);
                    n.pitch = if motif.frame == PitchFrame::Absolute {
                        checked_pitch(value)?
                    } else {
                        i16::try_from(value).map_err(|_| "Inversion overflow")?
                    };
                }
            }
            Transform::Retrograde => {
                for n in &mut motif.notes {
                    n.start = motif
                        .length
                        .checked_sub(n.start.checked_add(n.length).ok_or("Motif time overflow")?)
                        .ok_or("Motif event exceeds its cell")?;
                }
            }
            Transform::ScaleTime(a, b) => {
                let mut cell = RhythmSpec {
                    custom_length: motif.length,
                    custom: motif
                        .notes
                        .iter()
                        .map(|n| Pulse {
                            id: n.id,
                            start: n.start,
                            length: n.length,
                            velocity: n.velocity,
                            tied: false,
                        })
                        .collect(),
                    ..RhythmSpec::default()
                };
                super::rhythm::scale(&mut cell, a, b)?;
                motif.length = cell.custom_length;
                for (note, pulse) in motif.notes.iter_mut().zip(cell.custom) {
                    note.start = pulse.start;
                    note.length = pulse.length;
                }
            }
            Transform::Rotate(by) => {
                for n in &mut motif.notes {
                    n.start = (u64::from(n.start) + u64::from(by))
                        .rem_euclid(u64::from(motif.length)) as u32;
                    if n.start + n.length > motif.length {
                        return Err("Rotated sustain crosses the cell boundary; shorten it explicitly or extend the cell".into());
                    }
                }
            }
            Transform::Fragment(a, b) => {
                if a >= b || b > motif.length {
                    return Err("Fragment must be within the motif".into());
                }
                motif
                    .notes
                    .retain(|n| n.start < b && n.start + n.length > a);
                for n in &mut motif.notes {
                    let end = (n.start + n.length).min(b);
                    n.start = n.start.max(a) - a;
                    n.length = end - a - n.start;
                }
                motif.length = b - a;
            }
            Transform::Sequence { interval, repeats } => {
                if repeats == 0
                    || repeats > 32
                    || motif.notes.len() * usize::from(repeats) > MAX_EVENTS
                {
                    return Err("Sequence exceeds repetition budget".into());
                }
                let original = motif.clone();
                motif.notes.clear();
                motif.length = original
                    .length
                    .checked_mul(u32::from(repeats))
                    .filter(|n| *n <= MAX_TICKS)
                    .ok_or("Sequence exceeds timeline capacity")?;
                for repetition in 0..repeats {
                    for note in &original.notes {
                        let mut n = note.clone();
                        n.id = identity(&[note.id, u64::from(repetition)]);
                        n.start += original.length * u32::from(repetition);
                        let value =
                            i32::from(note.pitch) + i32::from(interval) * i32::from(repetition);
                        n.pitch = if motif.frame == PitchFrame::Absolute {
                            checked_pitch(value)?
                        } else {
                            i16::try_from(value).map_err(|_| "Sequence interval overflow")?
                        };
                        motif.notes.push(n);
                    }
                }
            }
        }
        motif.notes.sort_by_key(|n| (n.start, n.id));
    }
    Ok(motif)
}

pub fn place(c: &Composition, p: &Placement) -> Result<Vec<NoteEvent>, String> {
    let source = c
        .motifs
        .iter()
        .find(|m| m.id == p.motif)
        .ok_or("Placement's source motif is missing")?;
    let transformed = transform(source, &p.transforms, c.key_at(p.start))?;
    let mut notes = Vec::new();
    for n in &transformed.notes {
        let start = p
            .start
            .checked_add(n.start)
            .ok_or("Placement time overflow")?;
        if n.length == 0 || start.checked_add(n.length).is_none_or(|end| end > c.length) {
            return Err(
                "Placement exceeds composition; extend the composition or fragment the motif"
                    .into(),
            );
        }
        let harmony = c.harmony_at(start);
        let pitch = match transformed.frame {
            PitchFrame::Absolute => checked_pitch(i32::from(n.pitch))? as u8,
            PitchFrame::Chromatic => checked_pitch(i32::from(p.anchor) + i32::from(n.pitch))? as u8,
            PitchFrame::Diatonic => c
                .key_at(start)
                .ok_or("Scale-step motif needs a key")?
                .step(p.anchor, n.pitch)?,
            PitchFrame::ChordRoles => {
                let h = harmony.ok_or("Chord-role motif needs sounding harmony")?;
                let mut members = h.material.members.iter().collect::<Vec<_>>();
                members.sort_by_key(|m| (m.offset, m.id));
                if members.is_empty() {
                    return Err("Chord-role motif cannot address an empty collection".into());
                }
                let index = i32::from(n.pitch);
                let pc = members[index.rem_euclid(members.len() as i32) as usize].pc;
                let value = i32::from(p.anchor / 12) * 12
                    + i32::from(pc)
                    + index.div_euclid(members.len() as i32) * 12;
                checked_pitch(value)? as u8
            }
        };
        let mut provenance = Provenance::new(
            "motif.placement",
            harmony.map(|h| h.id),
            format!("{} · {}", source.name, transformed.frame.label()),
        );
        provenance.motif = Some(source.id);
        provenance.placement = Some(p.id);
        provenance.transforms = p.transforms.iter().map(Transform::label).collect();
        notes.push(NoteEvent {
            id: identity(&[0x6d6f746966, p.id, n.id]),
            voice: p.voice,
            member: None,
            pitch,
            start,
            length: n.length,
            velocity: n.velocity,
            provenance,
        });
    }
    Ok(notes)
}

pub fn capture(
    c: &mut Composition,
    events: &[NoteEvent],
    voice: crate::midi_lab::Voice,
    start: u32,
    end: u32,
    name: String,
) -> Result<u64, String> {
    if start >= end || end > c.length {
        return Err("Choose a valid capture span".into());
    }
    let notes = events
        .iter()
        .filter(|n| n.voice == voice && n.start >= start && n.end() <= end)
        .map(|n| MotifNote {
            id: n.id,
            pitch: i16::from(n.pitch),
            start: n.start - start,
            length: n.length,
            velocity: n.velocity,
        })
        .collect::<Vec<_>>();
    if notes.is_empty() {
        return Err("No complete notes in the selected capture span".into());
    }
    let id = c.mint();
    c.motifs.push(Motif {
        id,
        name,
        frame: PitchFrame::Absolute,
        length: end - start,
        notes,
    });
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cell() -> Motif {
        Motif {
            id: 1,
            name: "Cell".into(),
            frame: PitchFrame::Absolute,
            length: 96,
            notes: vec![
                MotifNote {
                    id: 1,
                    pitch: 60,
                    start: 0,
                    length: 24,
                    velocity: 90,
                },
                MotifNote {
                    id: 2,
                    pitch: 64,
                    start: 48,
                    length: 24,
                    velocity: 90,
                },
            ],
        }
    }
    #[test]
    fn inverse_operations_preserve_notes_and_identity() {
        let m = cell();
        assert_eq!(
            transform(&m, &[Transform::Invert(60), Transform::Invert(60)], None).unwrap(),
            m
        );
        assert_eq!(
            transform(&m, &[Transform::Retrograde, Transform::Retrograde], None).unwrap(),
            m
        );
        assert_eq!(
            transform(
                &m,
                &[Transform::ScaleTime(2, 1), Transform::ScaleTime(1, 2)],
                None
            )
            .unwrap(),
            m
        );
    }
    #[test]
    fn transformations_refuse_unrepresentable_or_out_of_range_material() {
        let m = cell();
        assert!(transform(&m, &[Transform::Transpose(100)], None).is_err());
        assert!(transform(&m, &[Transform::ScaleTime(1, 7)], None).is_err());
        assert!(transform(&m, &[Transform::Diatonic(2)], None).is_err());
    }
    #[test]
    fn absolute_placements_do_not_follow_chord_changes() {
        let mut c = Composition::default();
        c.motifs.push(cell());
        let p = Placement {
            id: 10,
            motif: 1,
            voice: crate::midi_lab::Voice::Melody,
            start: 0,
            anchor: 48,
            transforms: vec![],
        };
        let a = place(&c, &p).unwrap();
        c.harmony[0].material = crate::theory::material::Material::parse("F#7").unwrap();
        let b = place(&c, &p).unwrap();
        assert_eq!(
            a.iter().map(|n| n.pitch).collect::<Vec<_>>(),
            b.iter().map(|n| n.pitch).collect::<Vec<_>>()
        );
    }
}

/// Entirely written voices bypass generative defaults, including during migration.
pub fn covers_voice(c: &Composition, voice: Voice) -> Result<bool, String> {
    let mut spans = Vec::new();
    for p in c.placements.iter().filter(|p| p.voice == voice) {
        let m = c
            .motifs
            .iter()
            .find(|m| m.id == p.motif)
            .ok_or("Missing motif")?;
        let length = transform(m, &p.transforms, c.key_at(p.start))?.length;
        spans.push((
            p.start,
            p.start
                .checked_add(length)
                .ok_or("Placement time overflow")?,
        ));
    }
    spans.sort();
    let mut end = 0;
    for (start, stop) in spans {
        if start > end {
            return Ok(false);
        }
        end = end.max(stop);
    }
    Ok(end >= c.length)
}

/// Change a motif's coordinate system without changing pitches at its reference.
pub fn reframe(
    motif: &mut Motif,
    frame: PitchFrame,
    anchor: u8,
    key: Option<&Key>,
    material: Option<&crate::theory::material::Material>,
) -> Result<(), String> {
    let members = material
        .map(|m| {
            let mut members = m.members.iter().collect::<Vec<_>>();
            members.sort_by_key(|m| (m.offset, m.id));
            members
        })
        .unwrap_or_default();
    let decode = |value: i16, frame: PitchFrame| -> Result<u8, String> {
        match frame {
            PitchFrame::Absolute => Ok(checked_pitch(i32::from(value))? as u8),
            PitchFrame::Chromatic => Ok(checked_pitch(i32::from(anchor) + i32::from(value))? as u8),
            PitchFrame::Diatonic => key.ok_or("Diatonic frame needs a key")?.step(anchor, value),
            PitchFrame::ChordRoles => {
                if members.is_empty() {
                    return Err("Chord-role frame needs members".into());
                }
                let pc = members[i32::from(value).rem_euclid(members.len() as i32) as usize].pc;
                Ok(checked_pitch(
                    i32::from(anchor / 12) * 12
                        + i32::from(pc)
                        + i32::from(value).div_euclid(members.len() as i32) * 12,
                )? as u8)
            }
        }
    };
    let mut notes = motif.notes.clone();
    for n in &mut notes {
        let pitch = decode(n.pitch, motif.frame)?;
        n.pitch = (-256i16..=256)
            .find(|v| decode(*v, frame).ok() == Some(pitch))
            .ok_or("A source pitch cannot be represented exactly in this frame")?;
    }
    motif.notes = notes;
    motif.frame = frame;
    Ok(())
}
