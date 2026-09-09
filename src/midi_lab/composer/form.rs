use super::*;
use crate::midi_lab::Voice;

struct Resolved<'a> {
    source: &'a Section,
    steps: Vec<(i16, bool)>,
    enabled: [bool; 5],
    simplify: bool,
}
fn resolve(c: &Composition, id: u64) -> Result<Resolved<'_>, String> {
    let mut path = Vec::new();
    let mut next = Some(id);
    let mut enabled = [true; 5];
    let mut simplify = false;
    while let Some(id) = next {
        if path.len() >= 256 || path.iter().any(|s: &&Section| s.id == id) {
            return Err("Section references contain a cycle".into());
        }
        let s = c
            .sections
            .iter()
            .find(|s| s.id == id)
            .ok_or("Form refers to a missing section")?;
        if s.length == 0
            || s.start
                .checked_add(s.length)
                .is_none_or(|end| end > c.length)
        {
            return Err("Source section is outside the composition".into());
        }
        enabled = std::array::from_fn(|i| enabled[i] && s.enabled[i]);
        simplify |= s.simplify_bass;
        next = s.source;
        path.push(s);
    }
    let source = *path.last().ok_or("Missing section")?;
    if path.iter().any(|s| s.length != source.length) {
        return Err(
            "A section reference must preserve its source length; fragment the source explicitly"
                .into(),
        );
    }
    let steps = path
        .iter()
        .rev()
        .map(|s| (s.transpose, s.diatonic))
        .collect();
    Ok(Resolved {
        source,
        steps,
        enabled,
        simplify,
    })
}
fn transpose(c: &Composition, pitch: u8, at: u32, steps: &[(i16, bool)]) -> Result<u8, String> {
    let mut pitch = pitch;
    for &(amount, diatonic) in steps {
        if amount == 0 {
            continue;
        }
        pitch = if diatonic {
            c.key_at(at)
                .ok_or("Diatonic section change needs a key")?
                .step(pitch, amount)?
        } else {
            u8::try_from(i32::from(pitch) + i32::from(amount))
                .ok()
                .filter(|p| *p < 128)
                .ok_or("Section transposition exceeds MIDI range")?
        };
    }
    Ok(pitch)
}
pub fn expand(c: &Composition) -> Result<Composition, String> {
    for s in &c.sections {
        resolve(c, s.id)?;
    }
    Ok(c.clone())
}

pub fn render_form(c: &Composition) -> Result<Rendered, String> {
    let mut base = c.clone();
    base.form.clear();
    base.overrides.retain(|o| !o.instance);
    base.sections.clear();
    base.tension.clear();
    let base_output = super::render(&base)?;
    let mut out = Rendered {
        fingerprint: c.fingerprint()?,
        ..Rendered::default()
    };
    let mut start = 0u32;
    let mut occurrences = std::collections::BTreeMap::new();
    let mut source_to_instance = Vec::new();
    for id in &c.form {
        let instance = *occurrences.entry(*id).or_insert(0u64);
        *occurrences.get_mut(id).unwrap() += 1;
        let section = c
            .sections
            .iter()
            .find(|s| s.id == *id)
            .ok_or("Missing form section")?;
        let r = resolve(c, *id)?;
        let source = r.source;
        let stop = source.start + source.length;
        let end = start
            .checked_add(source.length)
            .filter(|end| *end <= MAX_TICKS)
            .ok_or("Expanded form exceeds timeline capacity")?;
        let hid = |id| identity(&[id, section.id, instance, 0x666f726d]);
        for h in base_output
            .harmony
            .iter()
            .filter(|h| h.start < stop && h.start + h.length > source.start)
        {
            if h.start < source.start || h.start + h.length > stop {
                return Err(
                    "A harmony span crosses a section boundary; split the harmony explicitly"
                        .into(),
                );
            }
            let mut next = h.clone();
            next.id = hid(h.id);
            next.start = start + h.start - source.start;
            if r.steps.iter().any(|(n, _)| *n != 0) {
                for m in &mut next.material.members {
                    let p = transpose(c, m.pitch.unwrap_or(60 + m.pc), h.start, &r.steps)?;
                    m.pc = p % 12;
                    if m.pitch.is_some() {
                        m.pitch = Some(p);
                    }
                    m.spelling = crate::theory::pitch_class_name(p).into();
                }
                next.material.root = next
                    .material
                    .root
                    .map(|p| transpose(c, 60 + p, h.start, &r.steps).map(|n| n % 12))
                    .transpose()?;
                next.material.bass = next
                    .material
                    .bass
                    .map(|b| match b {
                        crate::theory::material::Bass::Class(p) => {
                            transpose(c, 48 + p, h.start, &r.steps)
                                .map(|n| crate::theory::material::Bass::Class(n % 12))
                        }
                        crate::theory::material::Bass::Pitch(p) => {
                            transpose(c, p, h.start, &r.steps)
                                .map(crate::theory::material::Bass::Pitch)
                        }
                    })
                    .transpose()?;
                for m in &mut next.material.members {
                    m.offset = (i16::from(m.pc) - i16::from(next.material.root.unwrap_or(0)))
                        .rem_euclid(12);
                }
                next.material.source.clear();
            }
            if let Some(v) = base_output.voicings.iter().find(|v| v.harmony == h.id) {
                let mut v = v.clone();
                v.harmony = next.id;
                for (_, pitch) in &mut v.notes {
                    *pitch = transpose(c, *pitch, h.start, &r.steps)?;
                }
                v.runner_up = None;
                out.voicings.push(v);
            }
            out.harmony.push(next);
        }
        for n in base_output
            .notes
            .iter()
            .filter(|n| n.start < stop && n.end() > source.start)
        {
            if !r.enabled[n.voice.index()] {
                continue;
            }
            if n.start < source.start || n.end() > stop {
                return Err("A note crosses a source section boundary; extend the section or explicitly edit the sustain".into());
            }
            if r.simplify
                && n.voice == Voice::Bass
                && n.start % PPQ != 0
                && !c.overrides.iter().any(|o| o.id == n.id)
            {
                continue;
            }
            let mut next = n.clone();
            next.id = identity(&[n.id, *id, instance]);
            next.provenance.origin = Some(n.id);
            next.start = start + n.start - source.start;
            next.pitch = transpose(c, n.pitch, n.start, &r.steps)?;
            next.provenance.transforms.push(format!(
                "Section {} · source {} · {}",
                section.name,
                source.name,
                r.steps
                    .iter()
                    .map(|(n, d)| format!("{n:+} {}", if *d { "scale steps" } else { "semitones" }))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            next.provenance.harmony = n.provenance.harmony.map(hid);
            next.provenance.target = n
                .provenance
                .target
                .map(|p| transpose(c, p, n.start, &r.steps))
                .transpose()?;
            source_to_instance.push((n.id, next.id));
            out.notes.push(next);
        }
        start = end;
    }
    if out.notes.len() > MAX_EVENTS {
        return Err("Expanded form exceeds event budget".into());
    }
    out.length = start;
    let mut expanded = c.clone();
    expanded.length = start;
    expanded.form.clear();
    expanded.sections.clear();
    expanded.harmony = out.harmony.clone();
    // Source locks flow to every reference; local instance locks remain local.
    for (source, instance) in source_to_instance {
        if let Some(o) = c.overrides.iter().find(|o| o.id == source)
            && let Some(note) = out.notes.iter().find(|n| n.id == instance)
        {
            let inherited = Override {
                id: instance,
                instance: true,
                pitch: (o.pitch.is_some() || o.inserted.is_some()).then_some(note.pitch),
                start: o.start.map(|_| note.start),
                length: o.length.map(|_| note.length),
                velocity: o.velocity,
                ..Override::default()
            };
            if let Some(local) = expanded.overrides.iter_mut().find(|o| o.id == instance) {
                local.pitch = local.pitch.or(inherited.pitch);
                local.start = local.start.or(inherited.start);
                local.length = local.length.or(inherited.length);
                local.velocity = local.velocity.or(inherited.velocity);
            } else {
                expanded.overrides.push(inherited);
            }
        }
    }
    // Apply instance overrides only. Source overrides already ran before expansion.
    let mut local = expanded.clone();
    local
        .overrides
        .retain(|o| out.notes.iter().any(|n| n.id == o.id) || o.instance && o.inserted.is_some());
    super::generate::apply_overrides(&local, &mut out.notes, &mut out.findings)?;
    arrange(&expanded, &mut out.notes)?;
    out.notes
        .sort_by_key(|n| (n.start, n.voice.index(), n.pitch, n.id));
    super::generate::validate_events(&expanded, &out.notes, &mut out.findings)?;
    super::ornament::validate(&expanded, &out.notes)?;
    super::counterpoint::validate(&expanded, &out.notes, &mut out.findings)?;
    Ok(out)
}

pub fn arrange(c: &Composition, notes: &mut Vec<NoteEvent>) -> Result<(), String> {
    c.meter.bar()?;
    notes.retain(|n| {
        c.overrides.iter().any(|o| o.id == n.id && !o.deleted)
            || c.sections
                .iter()
                .filter(|s| {
                    s.start <= n.start && s.start + s.length > n.start && s.source.is_none()
                })
                .all(|s| {
                    s.enabled[n.voice.index()]
                        && (!s.simplify_bass || n.voice != Voice::Bass || n.start % PPQ == 0)
                })
    });
    if c.tension_map.density > 0 {
        notes.retain(|n| {
            if c.overrides.iter().any(|o| o.id == n.id)
                || !matches!(n.voice, Voice::Melody | Voice::Arp)
                || n.provenance.rule.contains("arrival")
                || n.start % PPQ == 0
            {
                return true;
            }
            let position = (u64::from(n.start) * 1000 / u64::from(c.length)) as u16;
            let tension = curve(&c.tension, position);
            let stride = 1 + u32::from(c.tension_map.density) * (100 - tension as u32) / 100;
            (n.start / (PPQ / 4)) % stride == 0
        });
    }
    for n in notes.iter_mut() {
        let position = (u64::from(n.start) * 1000 / u64::from(c.length)) as u16;
        let tension = i32::from(curve(&c.tension, position));
        let edit = c.overrides.iter().find(|o| o.id == n.id);
        if !edit.is_some_and(|o| o.velocity.is_some()) {
            n.velocity = (i32::from(n.velocity)
                + (tension - 50) * i32::from(c.tension_map.velocity) / 50)
                .clamp(1, 127) as u8;
        }
        let written = n.provenance.motif.is_some()
            || c.harmony_at(n.start)
                .is_some_and(|h| h.material.members.iter().any(|m| m.pitch.is_some()));
        if c.tension_map.register > 0
            && !written
            && !edit.is_some_and(|o| o.pitch.is_some() || o.inserted.is_some())
        {
            let octaves = (tension - 50) * i32::from(c.tension_map.register) / 50;
            let moved = i32::from(n.pitch) + 12 * octaves;
            let spec = &c.voices[n.voice.index()];
            if moved < i32::from(spec.low) || moved > i32::from(spec.high) {
                return Err("Tension register mapping exceeds a voice range; reduce the mapping or widen the range".into());
            }
            n.pitch = moved as u8;
        }
        if c.tension_map.gate > 0
            && !edit.is_some_and(|o| o.length.is_some())
            && !n
                .provenance
                .transforms
                .iter()
                .any(|t| t.starts_with("Tied through"))
        {
            let factor = 100 + (tension - 50) * i32::from(c.tension_map.gate) / 50;
            n.length = (u64::from(n.length) * factor as u64 / 100)
                .max(1)
                .min(u64::from(c.length - n.start)) as u32;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sections() -> Composition {
        let mut c = Composition::default();
        c.sections = vec![
            Section {
                id: 10,
                name: "A".into(),
                start: 0,
                length: 384,
                source: None,
                transpose: 0,
                diatonic: false,
                enabled: [true; 5],
                simplify_bass: false,
            },
            Section {
                id: 11,
                name: "A prime".into(),
                start: 0,
                length: 384,
                source: Some(10),
                transpose: 2,
                diatonic: false,
                enabled: [true; 5],
                simplify_bass: false,
            },
        ];
        c.form = vec![10, 11, 10];
        c
    }
    #[test]
    fn repeated_sections_reference_and_transform_source() {
        let c = sections();
        let r = render_form(&c).unwrap();
        assert_eq!(r.length, 1152);
        let first = r.notes.iter().find(|n| n.start == 0).unwrap();
        let second = r.notes.iter().find(|n| n.start == 384).unwrap();
        assert_eq!(first.pitch + 2, second.pitch);
        assert_ne!(first.id, second.id);
        assert!(!r.voicings.is_empty());
    }
    #[test]
    fn cycles_and_boundary_sustains_refuse() {
        let mut c = sections();
        c.sections[0].source = Some(10);
        assert!(render_form(&c).is_err());
        let mut c = sections();
        c.sections[0].length = 200;
        c.form = vec![10];
        assert!(render_form(&c).is_err());
    }
    #[test]
    fn local_note_edit_survives_a_source_change() {
        let mut c = sections();
        let r = render(&c).unwrap();
        let n = r.notes.iter().find(|n| n.start == 384).unwrap();
        let mut edited = n.clone();
        edited.velocity = 37;
        c.pin(&edited);
        c.harmony[0].voicing.center = 65;
        let updated = render(&c).unwrap();
        assert_eq!(
            updated
                .notes
                .iter()
                .find(|n| n.id == n.id && n.id == edited.id)
                .unwrap()
                .velocity,
            37
        );
    }
}
