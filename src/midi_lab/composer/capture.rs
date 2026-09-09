use super::*;
use crate::{midi_lab::Voice, theory::material::Material};

pub fn material(pitches: &[u8]) -> Result<Material, String> {
    if pitches.is_empty() || pitches.len() > 128 || pitches.iter().any(|p| *p > 127) {
        return Err("Capture must contain 1–128 valid MIDI pitches".into());
    }
    Material::parse(&format!(
        "notes:{}",
        pitches
            .iter()
            .map(|p| format!(
                "{}{}",
                crate::theory::pitch_class_name(*p),
                i16::from(*p) / 12 - 1
            ))
            .collect::<Vec<_>>()
            .join(",")
    ))
}

pub fn clip(pattern: &crate::sequencing::Pattern, key: &crate::pitch::Key) -> Vec<NoteEvent> {
    let mut notes = Vec::new();
    for i in 0..pattern.step_count() {
        let trig = pattern.trig(i);
        if !trig.enabled {
            continue;
        }
        for (member, n) in trig.notes.iter().enumerate().filter(|(_, n)| !n.muted) {
            let start = (i * crate::sequencing::PATTERN_STEP_TICKS)
                .saturating_add_signed(n.micro_ticks as isize) as u32;
            notes.push(NoteEvent {
                id: identity(&[pattern.id.0, i as u64, member as u64]),
                voice: Voice::Melody,
                member: None,
                pitch: crate::pitch::nearest_midi(n.pitch.resolve(key)),
                start,
                length: n.length_ticks as u32,
                velocity: n.velocity,
                provenance: Provenance::new(
                    "capture.clip",
                    None,
                    format!("Lifted from {}", pattern.tag),
                ),
            });
        }
    }
    notes.sort_by_key(|n| (n.start, n.pitch, n.id));
    notes
}

/// Import only exactly representable written notes. Performance operators need
/// an explicit rendered take before their audible result can become a motif.
pub fn checked_clip(
    pattern: &crate::sequencing::Pattern,
    key: &crate::pitch::Key,
) -> Result<Vec<NoteEvent>, String> {
    if pattern.swing != 50 || pattern.scale != crate::sequencing::Scale::One {
        return Err("Clip lifting requires straight timing at 1× speed; render its performance to notes first".into());
    }
    for i in 0..pattern.step_count() {
        let t = pattern.trig(i);
        if !t.enabled {
            continue;
        }
        if t.cond.is_some() || t.retrig.is_some() || t.probability != 1.0 {
            return Err("Clip lifting requires unconditional notes without retriggers; render its performance first".into());
        }
        for n in t.notes.iter().filter(|n| !n.muted) {
            let hz = n.pitch.resolve(key);
            let pitch = crate::pitch::nearest_midi(hz);
            if !hz.is_finite()
                || hz <= 0.
                || (1200. * (hz / crate::pitch::midi_to_hz(pitch)).log2()).abs() > 0.0001
            {
                return Err("Clip contains pitches outside exact 12-TET; lifting would require an explicit pitch conversion".into());
            }
            let start =
                (i * crate::sequencing::PATTERN_STEP_TICKS) as i64 + i64::from(n.micro_ticks);
            if start < 0 || start + n.length_ticks as i64 > i64::from(MAX_TICKS) {
                return Err("Clip note timing is outside the composition capacity".into());
            }
        }
    }
    let notes = clip(pattern, key);
    if notes.len() > MAX_EVENTS {
        return Err("Clip exceeds the composition event capacity".into());
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifting_refuses_lossy_pitch_or_performance_conversion() {
        let key = crate::pitch::default_key();
        let mut p =
            crate::sequencing::Pattern::empty(crate::sequencing::PatternId(1), "source".into());
        p.trig_mut(0)
            .add_tone_at(crate::sequencing::Note::new(60, 24, 80));
        assert_eq!(checked_clip(&p, &key).unwrap()[0].pitch, 60);
        p.trig_mut(0).notes[0].pitch.offset_cents = 25.;
        assert!(checked_clip(&p, &key).unwrap_err().contains("12-TET"));
        p.trig_mut(0).notes[0].pitch.offset_cents = 0.;
        p.swing = 60;
        assert!(checked_clip(&p, &key).unwrap_err().contains("straight"));
    }
    #[test]
    fn captured_unisons_and_octaves_keep_separate_members() {
        let m = material(&[36, 60, 60, 67]).unwrap();
        assert_eq!(
            m.realise(0, 127, 48)
                .unwrap()
                .iter()
                .map(|n| n.1)
                .collect::<Vec<_>>(),
            [36, 60, 60, 67]
        );
        assert_eq!(m.root, None);
    }
}
