use super::*;
use crate::midi_lab::{Event, Generated, Recipe, Voice};
use crate::theory::{harmony::Tone, material::Material};

pub fn generated(c: &Composition) -> Result<Generated, String> {
    let rendered = render(c)?;
    let notes = rendered
        .notes
        .iter()
        .map(|n| Event {
            id: n.id,
            voice: n.voice,
            pitch: n.pitch,
            start: n.start as usize,
            length: n.length as usize,
            velocity: n.velocity,
        })
        .collect();
    let voicings = rendered
        .voicings
        .iter()
        .map(|v| {
            (
                v.harmony,
                v.notes
                    .iter()
                    .map(|(id, pitch)| {
                        let member = rendered
                            .harmony
                            .iter()
                            .find(|h| h.id == v.harmony)
                            .and_then(|h| h.material.members.iter().find(|m| m.id == *id));
                        Tone {
                            pitch: *pitch,
                            degree: member.and_then(|m| m.degree).unwrap_or(0),
                            label: member.map_or_else(
                                || crate::theory::pitch_class_name(*pitch).into(),
                                |m| m.spelling.clone(),
                            ),
                            extension: member.and_then(|m| m.degree).is_some_and(|d| d >= 9),
                        }
                    })
                    .collect(),
            )
        })
        .collect();
    Ok(Generated {
        notes,
        voicings,
        warnings: rendered.findings.iter().map(|f| f.detail.clone()).collect(),
    })
}

/// Preserve the exact old result before adopting any new construction rules.
pub fn migrate(recipe: &Recipe) -> Result<Composition, String> {
    if let Some(c) = &recipe.composition {
        return Ok((**c).clone());
    }
    let old = crate::midi_lab::generate(recipe)?;
    let mut c = Composition {
        length: recipe.length as u32,
        harmony: vec![],
        key: None,
        ..Composition::default()
    };
    for h in &recipe.harmony {
        let material = Material::parse(&h.symbol)?;
        c.harmony.push(HarmonySpan {
            id: h.id,
            start: h.start as u32,
            length: h.length as u32,
            material,
            voicing: Voicing {
                layout: h.voicing.layout,
                center: ((h.voicing.octave + 1) * 12).clamp(0, 127) as u8,
                lead: h.voicing.lead,
                inversion: h.voicing.inversion,
                ..Voicing::default()
            },
            operation: Some("Imported legacy harmony; exact output retained".into()),
            replaced: None,
        });
    }
    for voice in Voice::ALL {
        let old = &recipe.voices[voice.index()];
        let next = &mut c.voices[voice.index()];
        next.enabled = old.enabled;
        next.profile = Some(recipe.style);
        next.low = old.low;
        next.high = old.high;
        next.velocity = old.velocity;
        next.rhythm.gate = old.gate;
        next.rhythm.swing = old.swing;
        next.rhythm.kind = match old.rhythm {
            crate::midi_lab::Rhythm::Hold => RhythmKind::Hold,
            crate::midi_lab::Rhythm::Quarter => RhythmKind::Quarter,
            crate::midi_lab::Rhythm::Eighth => RhythmKind::Eighth,
            crate::midi_lab::Rhythm::Sixteenth => RhythmKind::Sixteenth,
            crate::midi_lab::Rhythm::Triplet => RhythmKind::Triplet,
            crate::midi_lab::Rhythm::Syncopated => RhythmKind::Syncopated,
            crate::midi_lab::Rhythm::Euclidean => RhythmKind::Euclidean,
            crate::midi_lab::Rhythm::Custom => RhythmKind::Custom,
        };
        next.rhythm.steps = old.steps;
        next.rhythm.pulses = old.pulses;
        next.rhythm.rotation = u16::from(old.rotation);
        next.rhythm.custom_length = c.length;
        next.rhythm.custom = old
            .custom
            .iter()
            .map(|g| Pulse {
                id: g.id,
                start: g.start as u32,
                length: g.length as u32,
                velocity: g.velocity,
                tied: false,
            })
            .collect();
    }
    let events = old
        .notes
        .iter()
        .map(|n| NoteEvent {
            id: n.id,
            voice: n.voice,
            member: None,
            pitch: n.pitch,
            start: n.start as u32,
            length: n.length as u32,
            velocity: n.velocity,
            provenance: Provenance::new(
                "legacy.snapshot",
                recipe.harmony_at(n.start).map(|h| h.id),
                "Preserved event from the legacy generator",
            ),
        })
        .collect();
    c.frozen = true;
    c.snapshot = Some(Snapshot {
        harmony: c.harmony.clone(),
        voicings: old
            .voicings
            .iter()
            .map(|(id, tones)| VoicingDecision {
                harmony: *id,
                notes: tones
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        (
                            c.harmony
                                .iter()
                                .find(|h| h.id == *id)
                                .and_then(|h| {
                                    h.material.members.iter().find(|m| m.pc == t.pitch % 12)
                                })
                                .map_or(i as u64 + 1, |m| m.id),
                            t.pitch,
                        )
                    })
                    .collect(),
                cost: Cost::default(),
                runner_up: None,
                candidates: 0,
                searched: 0,
            })
            .collect(),
        length: c.length,
        input: c.input()?,
        events,
        label: "Original legacy output".into(),
    });
    Ok(c)
}

pub fn thaw(c: &mut Composition) -> Result<(), String> {
    let snapshot = c
        .snapshot
        .as_ref()
        .ok_or("No saved material to adopt")?
        .clone();
    c.overrides.clear();
    c.placements.clear();
    if !snapshot.harmony.is_empty() {
        c.harmony = snapshot.harmony.clone();
    }
    c.form.clear();
    c.sections.clear();
    c.tension.clear();
    if snapshot.length > 0 {
        c.length = snapshot.length;
    }
    for spec in &mut c.voices {
        spec.articulation = Articulation::Normal;
    }
    // Written cells make the imported notes editable without reinterpreting them.
    for voice in Voice::ALL {
        let notes = snapshot
            .events
            .iter()
            .filter(|n| n.voice == voice)
            .map(|n| MotifNote {
                id: n.id,
                pitch: i16::from(n.pitch),
                start: n.start,
                length: n.length,
                velocity: n.velocity,
            })
            .collect::<Vec<_>>();
        if notes.is_empty() {
            continue;
        }
        let motif = c.mint();
        let placement = c.mint();
        c.motifs.push(Motif {
            id: motif,
            name: format!("Imported {}", voice.label()),
            frame: PitchFrame::Absolute,
            length: c.length,
            notes,
        });
        c.placements.push(Placement {
            id: placement,
            motif,
            voice,
            start: 0,
            anchor: 60,
            transforms: vec![],
        });
    }
    c.frozen = false;
    c.engine = ENGINE_VERSION;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_recipes_keep_exact_output_after_migration_and_serde() {
        let old = Recipe::default();
        let expected = crate::midi_lab::generate(&old).unwrap();
        let migrated = migrate(&old).unwrap();
        assert!(migrated.frozen);
        let loaded: Composition = ron::from_str(&ron::to_string(&migrated).unwrap()).unwrap();
        let actual = generated(&loaded).unwrap();
        assert_eq!(actual.notes, expected.notes);
    }
}
