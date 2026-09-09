//! Decorations connect actual neighbouring events; provenance is checked again
//! after note edits, so a pin cannot silently invalidate a named construction.
use super::*;
use crate::midi_lab::Voice;

fn locked(c: &Composition, id: u64) -> bool {
    c.overrides.iter().any(|o| o.id == id)
}
fn step(c: &Composition, at: u32, pitch: u8, direction: i16) -> Option<u8> {
    if let Some(key) = c.key_at(at) {
        key.step(pitch, direction).ok()
    } else {
        u8::try_from(i16::from(pitch) + direction)
            .ok()
            .filter(|p| *p < 128)
    }
}
fn mark(n: &mut NoteEvent, rule: &str, target: u8) {
    n.provenance.rule = format!(
        "{}.{rule}",
        if n.voice == Voice::Bass {
            "bass"
        } else {
            "melody"
        }
    );
    n.provenance.target = Some(target);
    n.provenance
        .detail
        .push_str(&format!(" · {rule} resolves to MIDI {target}"));
}
pub fn decorate(
    c: &Composition,
    voice: Voice,
    kind: Decoration,
    notes: &mut Vec<NoteEvent>,
) -> Result<(), String> {
    let spec = &c.voices[voice.index()];
    let valid = |p: u8| p >= spec.low && p <= spec.high;
    let mut i = 1;
    while i + 1 < notes.len() {
        if locked(c, notes[i].id) {
            i += 1;
            continue;
        }
        let before = notes[i - 1].pitch;
        let after = notes[i + 1].pitch;
        let at = notes[i].start;
        let weak = at % PPQ != 0
            || voice == Voice::Bass
                && (notes[i].provenance.harmony != notes[i + 1].provenance.harmony
                    || notes
                        .get(i + 2)
                        .is_some_and(|n| n.provenance.harmony != notes[i].provenance.harmony));
        let boundary = notes[i].provenance.harmony != notes[i + 1].provenance.harmony;
        match kind {
            Decoration::Passing if weak => {
                let direction = (i16::from(after) - i16::from(before)).signum();
                if let Some(p) = step(c, at, before, direction).filter(|p| {
                    valid(*p)
                        && p.abs_diff(after) <= 2
                        && *p != after
                        && *p != before
                        && (i16::from(after) - i16::from(*p)).signum() == direction
                }) {
                    notes[i].pitch = p;
                    mark(&mut notes[i], "passing", after);
                }
            }
            Decoration::Neighbour if weak && before == after => {
                if let Some(p) = step(
                    c,
                    at,
                    before,
                    if spec.melody.variation % 2 == 0 {
                        1
                    } else {
                        -1
                    },
                )
                .filter(|p| valid(*p))
                {
                    notes[i].pitch = p;
                    mark(&mut notes[i], "neighbour", after);
                }
            }
            Decoration::Chromatic if weak || boundary => {
                if let Ok(p) = u8::try_from(
                    i16::from(after)
                        + if spec.melody.variation % 2 == 0 {
                            -1
                        } else {
                            1
                        },
                ) {
                    if valid(p) {
                        notes[i].pitch = p;
                        mark(&mut notes[i], "chromatic-approach", after);
                    }
                }
            }
            Decoration::Anticipation if boundary => {
                notes[i].pitch = after;
                mark(&mut notes[i], "anticipation", after);
            }
            Decoration::Escape if weak => {
                let direction = (i16::from(before) - i16::from(after)).signum();
                if let Some(p) =
                    step(c, at, before, direction).filter(|p| valid(*p) && p.abs_diff(after) >= 3)
                {
                    notes[i].pitch = p;
                    mark(&mut notes[i], "escape", after);
                }
            }
            Decoration::Enclosure if weak && i + 2 < notes.len() && !locked(c, notes[i + 1].id) => {
                let target = notes[i + 2].pitch;
                if target > spec.low && target < spec.high {
                    notes[i].pitch = target + 1;
                    notes[i + 1].pitch = target - 1;
                    mark(&mut notes[i], "enclosure-upper", target);
                    mark(&mut notes[i + 1], "enclosure-lower", target);
                    i += 2;
                }
            }
            Decoration::Suspension
                if notes[i - 1].provenance.harmony != notes[i].provenance.harmony
                    && !locked(c, notes[i - 1].id) =>
            {
                let harmonic = c
                    .harmony_at(at)
                    .is_some_and(|h| h.material.mask() & (1 << (after % 12)) != 0);
                if before > after && before - after <= 2 && harmonic {
                    let end = notes[i + 1].start;
                    notes[i - 1].length = end - notes[i - 1].start;
                    mark(&mut notes[i - 1], "suspension", after);
                    notes.remove(i);
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

pub fn validate(c: &Composition, events: &[NoteEvent]) -> Result<(), String> {
    for voice in Voice::ALL {
        let mut notes = events
            .iter()
            .filter(|n| n.voice == voice)
            .collect::<Vec<_>>();
        notes.sort_by_key(|n| (n.start, n.id));
        for (i, n) in notes.iter().enumerate() {
            let rule = n.provenance.rule.as_str();
            let Some(next) = notes.get(i + 1).copied().or_else(|| {
                if c.looping {
                    notes.first().copied()
                } else {
                    None
                }
            }) else {
                continue;
            };
            let before = i.checked_sub(1).map(|i| notes[i].pitch);
            let pitch = n.pitch;
            let after = next.pitch;
            let a = before.map(|b| i16::from(pitch) - i16::from(b));
            let b = i16::from(after) - i16::from(pitch);
            let valid = match rule {
                "melody.passing" => a.is_some_and(|a| {
                    a != 0 && a.abs() <= 2 && b != 0 && b.abs() <= 2 && a.signum() == b.signum()
                }),
                "melody.neighbour" | "bass.neighbour" => {
                    before == Some(after) && pitch.abs_diff(after) > 0 && pitch.abs_diff(after) <= 2
                }
                "melody.escape" | "bass.escape" => a.is_some_and(|a| {
                    a != 0 && a.abs() <= 2 && b.abs() >= 3 && a.signum() != b.signum()
                }),
                "melody.chromatic-approach" | "bass.chromatic-approach" => {
                    pitch.abs_diff(after) == 1
                }
                "bass.step-approach" => pitch.abs_diff(after) > 0 && pitch.abs_diff(after) <= 2,
                "melody.anticipation" | "bass.anticipation" => pitch == after,
                "melody.suspension" | "bass.suspension" => {
                    pitch > after && pitch - after <= 2 && n.end() == next.start
                }
                "melody.enclosure-upper" | "bass.enclosure-upper" => {
                    notes.get(i + 2).is_some_and(|target| {
                        i16::from(pitch) == i16::from(target.pitch) + 1
                            && i16::from(after) + 1 == i16::from(target.pitch)
                    })
                }
                "melody.enclosure-lower" | "bass.enclosure-lower" => {
                    i16::from(pitch) + 1 == i16::from(after)
                }
                _ => true,
            };
            if !valid {
                return Err(format!(
                    "{} at tick {} no longer has its required approach/resolution; edit its neighbouring notes or choose another decoration",
                    rule, n.start
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn passage(pitches: &[u8]) -> Vec<NoteEvent> {
        pitches
            .iter()
            .enumerate()
            .map(|(i, p)| NoteEvent {
                id: i as u64 + 10,
                voice: Voice::Melody,
                member: None,
                pitch: *p,
                start: i as u32 * 24,
                length: 24,
                velocity: 90,
                provenance: Provenance::new("melody.chord-member", Some(1), ""),
            })
            .collect()
    }
    #[test]
    fn passing_and_neighbour_have_real_resolutions() {
        let c = Composition::default();
        let mut n = passage(&[60, 60, 64]);
        decorate(&c, Voice::Melody, Decoration::Passing, &mut n).unwrap();
        assert_eq!(n[1].pitch, 62);
        validate(&c, &n).unwrap();
        n[2].pitch = 67;
        assert!(validate(&c, &n).is_err());
        let mut n = passage(&[60, 64, 60]);
        decorate(&c, Voice::Melody, Decoration::Neighbour, &mut n).unwrap();
        assert_eq!(n[1].pitch, 62);
        validate(&c, &n).unwrap();
    }
}
