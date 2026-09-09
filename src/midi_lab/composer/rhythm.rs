use super::*;
use crate::midi_lab::Voice;

pub fn pulses(c: &Composition, voice: Voice) -> Result<Vec<Pulse>, String> {
    c.validate()?;
    let spec = &c.voices[voice.index()];
    let rhythm = &spec.rhythm;
    let bar = c.meter.bar()?;
    let kind = if voice == Voice::Bass && spec.bass.role == BassRole::Walking {
        RhythmKind::Quarter
    } else if voice == Voice::Bass && spec.bass.role == BassRole::Sub {
        RhythmKind::Hold
    } else {
        rhythm.kind
    };
    let mut result = Vec::new();
    if kind == RhythmKind::Hold {
        for h in &c.harmony {
            result.push(Pulse {
                id: h.id,
                start: h.start,
                length: (h.length * u32::from(rhythm.gate) / 100).max(1),
                velocity: spec.velocity,
                tied: false,
            });
        }
    } else if kind == RhythmKind::Custom {
        if rhythm.custom_length == 0 || rhythm.custom_length > MAX_TICKS {
            return Err("Written rhythm needs a positive cell length".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for pulse in &rhythm.custom {
            if !ids.insert(pulse.id)
                || pulse.length == 0
                || pulse.start >= rhythm.custom_length
                || pulse.velocity == 0
                || pulse.velocity > 127
                || pulse
                    .start
                    .checked_add(pulse.length)
                    .is_none_or(|n| n > rhythm.custom_length)
            {
                return Err("Written rhythm contains an invalid or duplicate event".into());
            }
        }
        for repetition in 0..c.length.div_ceil(rhythm.custom_length) {
            for pulse in &rhythm.custom {
                if result.len() >= MAX_EVENTS {
                    return Err("Rhythm exceeds event budget".into());
                }
                let rotated = (pulse.start + u32::from(rhythm.rotation)) % rhythm.custom_length;
                let start = repetition * rhythm.custom_length + rotated;
                if start < c.length {
                    result.push(Pulse {
                        id: if repetition == 0 {
                            pulse.id
                        } else {
                            identity(&[repetition as u64, pulse.id])
                        },
                        start,
                        length: pulse.length.min(c.length - start),
                        velocity: pulse.velocity,
                        tied: pulse.tied,
                    });
                }
            }
        }
    } else {
        let step = match kind {
            RhythmKind::Quarter => PPQ,
            RhythmKind::Eighth | RhythmKind::Offbeat => PPQ / 2,
            RhythmKind::Triplet => PPQ / 3,
            _ => PPQ / 4,
        };
        for (i, start) in (0..c.length).step_by(step as usize).enumerate() {
            let index = i + usize::from(rhythm.rotation);
            let include = match kind {
                RhythmKind::Syncopated => [0, 3, 6, 10, 12, 15].contains(&(index % 16)),
                RhythmKind::Tresillo => [0, 6, 12].contains(&(index % 16)),
                RhythmKind::Offbeat => index % 2 == 1,
                RhythmKind::Euclidean => {
                    ((index % usize::from(rhythm.steps)) * usize::from(rhythm.pulses))
                        % usize::from(rhythm.steps)
                        < usize::from(rhythm.pulses)
                }
                _ => true,
            };
            if !include {
                continue;
            }
            let swing = if kind != RhythmKind::Triplet && i % 2 == 1 {
                step * u32::from(rhythm.swing - 50) / 50
            } else {
                0
            };
            let offset = if rhythm.offsets.is_empty() {
                0
            } else {
                i32::from(rhythm.offsets[i % rhythm.offsets.len()])
            };
            let shifted = i64::from(start + swing) + i64::from(offset);
            if shifted < 0 || shifted >= i64::from(c.length) {
                return Err("Timing offset moves an attack outside the composition".into());
            }
            let accent = if rhythm.accents.is_empty() {
                0
            } else {
                i16::from(rhythm.accents[i % rhythm.accents.len()])
            };
            let velocity = (i16::from(spec.velocity) + accent).clamp(1, 127) as u8;
            let start = shifted as u32;
            result.push(Pulse {
                id: identity(&[
                    u64::from((i as u32 * step) / bar),
                    u64::from((i as u32 * step) % bar),
                ]),
                start,
                length: (step * u32::from(rhythm.gate) / 100)
                    .max(1)
                    .min(c.length - start),
                velocity,
                tied: false,
            });
        }
    }
    if voice == Voice::Bass && spec.bass.groove != Groove::Independent {
        let mut kicks = Vec::new();
        for repetition in 0..c.length.div_ceil(spec.bass.kick_length) {
            for &at in &spec.bass.kick {
                if at >= spec.bass.kick_length {
                    return Err("Kick attack exceeds its rhythm cell".into());
                }
                let start = repetition * spec.bass.kick_length + at;
                if start < c.length {
                    kicks.push(start);
                }
            }
        }
        kicks.sort();
        kicks.dedup();
        if spec.bass.groove == Groove::Reinforce {
            result = kicks
                .into_iter()
                .map(|start| Pulse {
                    id: identity(&[0x6b69636b, u64::from(start)]),
                    start,
                    length: (PPQ / 2 * u32::from(rhythm.gate) / 100)
                        .max(1)
                        .min(c.length - start),
                    velocity: spec.velocity,
                    tied: false,
                })
                .collect();
        } else {
            result.retain(|p| !kicks.contains(&p.start));
        }
    }
    result.sort_by_key(|p| (p.start, p.id));
    if result.len() > MAX_EVENTS {
        return Err("Rhythm exceeds event budget".into());
    }
    Ok(result)
}

pub fn scale(cell: &mut RhythmSpec, numerator: u16, denominator: u16) -> Result<(), String> {
    if numerator == 0 || denominator == 0 {
        return Err("Time ratio must be positive".into());
    }
    let exact = |n: u32| -> Result<u32, String> {
        let v = u64::from(n) * u64::from(numerator);
        if v % u64::from(denominator) != 0 {
            return Err("Ratio cannot be represented exactly at 48 ticks per beat".into());
        }
        u32::try_from(v / u64::from(denominator)).map_err(|_| "Time ratio exceeds capacity".into())
    };
    let mut next = cell.clone();
    next.custom_length = exact(next.custom_length)?;
    if next.custom_length == 0 || next.custom_length > MAX_TICKS {
        return Err("Scaled rhythm exceeds capacity".into());
    }
    for pulse in &mut next.custom {
        pulse.start = exact(pulse.start)?;
        pulse.length = exact(pulse.length)?;
    }
    *cell = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rhythm_is_independent_of_harmonic_boundaries() {
        let mut c = Composition::default();
        c.voices[2].rhythm.kind = RhythmKind::Eighth;
        let before = pulses(&c, Voice::Melody).unwrap();
        c.harmony[0].length = 96;
        c.harmony[1].start = 96;
        c.harmony[1].length = 288;
        assert_eq!(before, pulses(&c, Voice::Melody).unwrap());
    }
    #[test]
    fn offset_bounds_and_inexact_ratios_refuse() {
        let mut c = Composition::default();
        c.voices[2].rhythm.offsets = vec![-1];
        assert!(pulses(&c, Voice::Melody).is_err());
        let mut cell = RhythmSpec {
            custom_length: 7,
            ..RhythmSpec::default()
        };
        let before = cell.clone();
        assert!(scale(&mut cell, 1, 3).is_err());
        assert_eq!(before, cell);
    }
    #[test]
    fn kick_relationships_are_actual_onset_relationships() {
        let mut c = Composition::default();
        c.voices[3].bass.groove = Groove::Reinforce;
        let reinforce = pulses(&c, Voice::Bass).unwrap();
        assert!(reinforce.iter().all(|p| [0, 96].contains(&(p.start % 192))));
        c.voices[3].bass.groove = Groove::Answer;
        let answer = pulses(&c, Voice::Bass).unwrap();
        assert!(answer.iter().all(|p| ![0, 96].contains(&(p.start % 192))));
    }
}

/// A written tie means one continuous note, never a hidden retrigger.
pub fn tie(c: &Composition, events: &mut Vec<NoteEvent>) -> Result<(), String> {
    for voice in Voice::ALL {
        if !c.voices[voice.index()].enabled {
            continue;
        }
        let pulses = pulses(c, voice)?;
        for (i, p) in pulses.iter().enumerate().filter(|(_, p)| p.tied).rev() {
            let next=pulses.get(i+1).ok_or("A tie cannot cross the transport boundary; write one sustain within the composition")?;
            let source = events
                .iter()
                .filter(|n| n.voice == voice && n.start == p.start)
                .cloned()
                .collect::<Vec<_>>();
            for n in source {
                let continuation = events
                    .iter()
                    .find(|m| m.voice == voice && m.start == next.start && m.pitch == n.pitch)
                    .cloned()
                    .ok_or("A written tie needs the same pitch at its next attack")?;
                if c.overrides
                    .iter()
                    .any(|o| o.id == n.id || o.id == continuation.id)
                {
                    return Err("A written tie conflicts with locked note timing; adopt one explicit held note instead".into());
                }
                if let Some(source) = events.iter_mut().find(|m| m.id == n.id) {
                    source.length = continuation.end() - source.start;
                    source
                        .provenance
                        .transforms
                        .push(format!("Tied through source event {}", continuation.id));
                }
                events.retain(|m| m.id != continuation.id);
            }
        }
    }
    Ok(())
}
