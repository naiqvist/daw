use super::*;
use crate::{
    midi_lab::Voice,
    theory::material::{Bass, Material},
};

pub fn pool(material: &Material, low: u8, high: u8) -> Vec<u8> {
    (low..=high)
        .filter(|p| material.mask() & (1 << (p % 12)) != 0)
        .collect()
}
fn nearest(pitches: &[u8], target: i32) -> Result<u8, String> {
    pitches
        .iter()
        .copied()
        .min_by_key(|p| ((i32::from(*p) - target).abs(), *p))
        .ok_or_else(|| "No compatible pitch in the requested register".into())
}
fn root_pool(material: &Material, low: u8, high: u8) -> Result<Vec<u8>, String> {
    match material.bass {
        Some(Bass::Pitch(p)) => {
            if p >= low && p <= high {
                Ok(vec![p])
            } else {
                Err("Explicit bass is outside the bass instrument range".into())
            }
        }
        Some(Bass::Class(pc)) => Ok((low..=high).filter(|p| p % 12 == pc).collect()),
        None => {
            if let Some(root) = material.root {
                Ok((low..=high).filter(|p| p % 12 == root).collect())
            } else {
                Ok(pool(material, low, high))
            }
        }
    }
}
fn contour_target(c: &Composition, spec: &PartSpec, tick: u32) -> i32 {
    let bar = c.meter.bar().unwrap_or(192);
    let phrase = bar * u32::from(spec.melody.phrase_bars);
    let phase = tick % phrase;
    let x = phase.saturating_mul(1000) / phrase.max(1);
    let y = match spec.melody.contour {
        Contour::Rise => x,
        Contour::Fall => 1000 - x,
        Contour::Arch => {
            if x < 500 {
                x * 2
            } else {
                (1000 - x) * 2
            }
        }
        Contour::Valley => {
            if x < 500 {
                1000 - x * 2
            } else {
                (x - 500) * 2
            }
        }
        Contour::Plateau => 500,
        Contour::Waves => {
            if x % 500 < 250 {
                x % 250 * 4
            } else {
                1000 - x % 250 * 4
            }
        }
        Contour::Drawn => {
            (i32::from(curve(&spec.melody.curve, x as u16)).clamp(0, 100) * 10) as u32
        }
    };
    i32::from(spec.low) + (i32::from(spec.high) - i32::from(spec.low)) * y as i32 / 1000
}
fn target(
    c: &Composition,
    h: &HarmonySpan,
    spec: &PartSpec,
    previous: Option<&HarmonySpan>,
) -> Result<u8, String> {
    let mut pitches = pool(&h.material, spec.low, spec.high);
    match spec.melody.targets {
        TargetKind::Root => pitches = root_pool(&h.material, spec.low, spec.high)?,
        TargetKind::Third | TargetKind::Seventh => {
            let degree = if spec.melody.targets == TargetKind::Third {
                3
            } else {
                7
            };
            let pcs = h
                .material
                .members
                .iter()
                .filter(|m| m.degree == Some(degree))
                .map(|m| m.pc)
                .collect::<Vec<_>>();
            if pcs.is_empty() {
                return Err(format!(
                    "{} needs a defined degree {degree}; select members or an exact target",
                    h.material.label()
                ));
            }
            pitches.retain(|p| pcs.contains(&(p % 12)));
        }
        TargetKind::Common => {
            if let Some(previous) = previous {
                let common = pitches
                    .iter()
                    .copied()
                    .filter(|p| previous.material.mask() & (1 << (p % 12)) != 0)
                    .collect::<Vec<_>>();
                if !common.is_empty() {
                    pitches = common;
                }
            }
        }
        TargetKind::Exact => {
            let pitch = spec.melody.arrival.ok_or("Set an exact phrase arrival")?;
            if pitch < spec.low || pitch > spec.high {
                return Err("Exact arrival is outside the voice range".into());
            }
            return Ok(pitch);
        }
        TargetKind::Members => {}
    }
    let offset = [0, 2, -2, 4, -4, 7, -7, 12, -12][usize::from(spec.melody.variation) % 9];
    nearest(&pitches, contour_target(c, spec, h.start) + offset)
}

fn melodic(c: &Composition, voice: Voice) -> Result<Vec<NoteEvent>, String> {
    let spec = &c.voices[voice.index()];
    let pulses = rhythm::pulses(c, voice)?;
    let bar = c.meter.bar()?;
    let phrase = bar * u32::from(spec.melody.phrase_bars);
    let mut harmony = c.harmony.iter().collect::<Vec<_>>();
    harmony.sort_by_key(|h| (h.start, h.id));
    let mut targets = std::collections::BTreeMap::new();
    for (i, h) in harmony.iter().enumerate() {
        if !h.material.members.is_empty() {
            targets.insert(
                h.id,
                target(c, h, spec, i.checked_sub(1).map(|i| harmony[i]))?,
            );
        }
    }
    let mut result: Vec<NoteEvent> = Vec::new();
    let mut previous: Option<u8> = None;
    for (at, pulse) in pulses.iter().enumerate() {
        let Some(h) = c.harmony_at(pulse.start) else {
            continue;
        };
        if h.material.members.is_empty() {
            continue;
        }
        let pitches = pool(&h.material, spec.low, spec.high);
        let arrival = targets[&h.id];
        let phrase_end = pulses
            .get(at + 1)
            .is_none_or(|p| p.start / phrase != pulse.start / phrase);
        let boundary = at == 0
            || pulses
                .get(at - 1)
                .and_then(|p| c.harmony_at(p.start))
                .is_none_or(|old| old.id != h.id);
        let local = (pulse.start % bar) / (PPQ / 4);
        let answer = (pulse.start / bar) % u32::from(spec.melody.phrase_bars);
        let mut desired = contour_target(c, spec, pulse.start);
        let mut rule = "melody.chord-member";
        match spec.melody.development {
            Development::Sequence => desired += answer as i32 * i32::from(spec.melody.interval),
            Development::Invert if answer % 2 == 1 => {
                desired = i32::from(spec.low) + i32::from(spec.high) - desired
            }
            Development::Answer if answer % 2 == 1 => desired += if local < 8 { 2 } else { -2 },
            Development::Contrast if answer % 2 == 1 => {
                desired = i32::from(spec.low) + i32::from(spec.high) - desired + 5
            }
            Development::Fragment if answer % 2 == 1 && local >= 8 => continue,
            _ => {}
        }
        let before = previous.unwrap_or(arrival);
        let mut pitch = if boundary {
            rule = "melody.arrival";
            arrival
        } else {
            match spec.melody.movement {
                Movement::Repeated => nearest(&pitches, i32::from(before))?,
                Movement::Arpeggio => {
                    let pcs = h.material.members.iter().map(|m| m.pc).collect::<Vec<_>>();
                    let pc = pcs[(local as usize + usize::from(spec.melody.variation)) % pcs.len()];
                    nearest(
                        &pitches
                            .iter()
                            .copied()
                            .filter(|p| p % 12 == pc)
                            .collect::<Vec<_>>(),
                        desired,
                    )?
                }
                Movement::Sequence => nearest(
                    &pitches,
                    i32::from(before) + i32::from(spec.melody.interval),
                )?,
                Movement::LeapRecover => nearest(
                    &pitches,
                    i32::from(before)
                        + if local % 4 == 0 {
                            7
                        } else if local % 4 == 1 {
                            -2
                        } else {
                            (desired - i32::from(before)).signum() * 2
                        },
                )?,
                Movement::Steps => {
                    let direction = (desired - i32::from(before)).signum();
                    let moving = pitches
                        .iter()
                        .copied()
                        .filter(|p| {
                            *p != before
                                && (i32::from(*p) - i32::from(before)).signum() == direction
                        })
                        .collect::<Vec<_>>();
                    nearest(
                        if moving.is_empty() { &pitches } else { &moving },
                        i32::from(before) + direction * 2,
                    )?
                }
            }
        };
        let mut goal = arrival;
        if phrase_end && let Some(ending) = spec.melody.arrival {
            pitch = ending;
            goal = ending;
            rule = "melody.phrase-arrival";
        }
        if pitch < spec.low || pitch > spec.high {
            return Err("Melody target exceeds its register".into());
        }
        if previous.is_some_and(|p| p.abs_diff(pitch) > spec.melody.max_leap)
            && !boundary
            && rule == "melody.chord-member"
        {
            let feasible = pitches
                .iter()
                .copied()
                .filter(|p| p.abs_diff(before) <= spec.melody.max_leap)
                .collect::<Vec<_>>();
            pitch = nearest(&feasible, desired)?;
        }
        let mut provenance = Provenance::new(
            rule,
            Some(h.id),
            format!(
                "{} · {} · {}",
                spec.melody.contour.label(),
                spec.melody.movement.label(),
                spec.melody.development.label()
            ),
        );
        provenance.target = Some(goal);
        result.push(NoteEvent {
            id: identity(&[voice.index() as u64, h.id, pulse.id, 0x6d656c]),
            voice,
            member: None,
            pitch,
            start: pulse.start,
            length: pulse.length,
            velocity: pulse.velocity,
            provenance,
        });
        previous = Some(pitch);
    }
    develop(c, voice, &mut result)?;
    super::ornament::decorate(c, voice, spec.melody.decoration, &mut result)?;

    Ok(result)
}

/// Develop a phrase from its opening bar. The original skeleton remains the
/// source of attacks; only matching motif positions are transformed.
fn develop(c: &Composition, voice: Voice, notes: &mut Vec<NoteEvent>) -> Result<(), String> {
    let spec = &c.voices[voice.index()];
    let bar = c.meter.bar()?;
    let phrase = bar * u32::from(spec.melody.phrase_bars);
    let original = notes.clone();
    for n in notes.iter_mut() {
        let position = n.start % phrase;
        let repetition = position / bar;
        if repetition == 0 || n.provenance.rule == "melody.phrase-arrival" {
            continue;
        }
        let opening = n.start / phrase * phrase;
        let Some(source) = original
            .iter()
            .find(|s| s.start == opening + position % bar)
        else {
            continue;
        };
        let Some(anchor) = original
            .iter()
            .find(|s| s.start >= opening && s.start < opening + bar)
        else {
            continue;
        };
        let Some(h) = c.harmony_at(n.start) else {
            continue;
        };
        let pool = pool(&h.material, spec.low, spec.high);
        if pool.is_empty() {
            continue;
        }
        let interval = i32::from(source.pitch) - i32::from(anchor.pitch);
        let target = match spec.melody.development {
            Development::Repeat => i32::from(source.pitch),
            Development::Sequence => {
                i32::from(source.pitch) + repetition as i32 * i32::from(spec.melody.interval)
            }
            Development::Invert if repetition % 2 == 1 => i32::from(anchor.pitch) - interval,
            Development::Answer if repetition % 2 == 1 && position % bar >= bar / 2 => {
                i32::from(anchor.pitch) - interval + 2
            }
            Development::Contrast if repetition % 2 == 1 => {
                i32::from(spec.low) + i32::from(spec.high) - i32::from(source.pitch)
            }
            Development::Fragment => i32::from(source.pitch),
            _ => i32::from(source.pitch),
        };
        if n.provenance.rule == "melody.arrival"
            && !matches!(spec.melody.targets, TargetKind::Members)
        {
            continue;
        }
        n.pitch = nearest(&pool, target)?;
        n.provenance.transforms.push(format!(
            "{} from opening note {}",
            spec.melody.development.label(),
            source.id
        ));
        n.provenance.detail.push_str(&format!(
            " · opening interval {interval:+} resolves to MIDI {} in {}",
            n.pitch,
            h.material.label()
        ));
    }
    Ok(())
}

fn bassline(c: &Composition, melody: &[NoteEvent]) -> Result<Vec<NoteEvent>, String> {
    let voice = Voice::Bass;
    let spec = &c.voices[voice.index()];
    let bass = &spec.bass;
    let pulses = rhythm::pulses(c, voice)?;
    let bar = c.meter.bar()?;
    let mut targets = std::collections::BTreeMap::new();
    let mut previous = None;
    let mut spans = c.harmony.iter().collect::<Vec<_>>();
    spans.sort_by_key(|h| (h.start, h.id));
    for h in &spans {
        if h.material.members.is_empty() {
            continue;
        }
        let pool = root_pool(&h.material, spec.low, spec.high)?;
        let target = nearest(&pool, previous.unwrap_or(i32::from(spec.low) + 8))?;
        targets.insert(h.id, target);
        previous = Some(i32::from(target));
    }
    let mut result: Vec<NoteEvent> = Vec::new();
    for (i, pulse) in pulses.iter().enumerate() {
        let Some(h) = c.harmony_at(pulse.start) else {
            continue;
        };
        if h.material.members.is_empty() {
            continue;
        }
        let arrival = targets[&h.id];
        let beat = (pulse.start % bar) / PPQ;
        let phrase_bar = pulse.start / bar;
        let filling = bass.fill_every > 0
            && (phrase_bar + 1) % u32::from(bass.fill_every) == 0
            && pulse.start % bar >= bar.saturating_sub(u32::from(bass.fill_beats) * PPQ);
        if bass.leave_melody_space
            && filling
            && melody
                .iter()
                .any(|n| n.start <= pulse.start && n.end() > pulse.start)
        {
            continue;
        }
        let pool = pool(&h.material, spec.low, spec.high);
        let mut rule = "bass.harmonic-arrival";
        let previous = result.last().map_or(arrival, |n| n.pitch);
        let chord_members = h.material.members.iter().map(|m| m.pc).collect::<Vec<_>>();
        let mut pitch = match bass.role {
            BassRole::Pedal => {
                rule = "bass.pedal";
                bass.pedal
            }
            BassRole::Sub => {
                rule = "bass.sustain";
                arrival
            }
            BassRole::Foundation => {
                if filling {
                    rule = "bass.phrase-answer";
                    let pc = chord_members
                        [(beat as usize + usize::from(bass.variation)) % chord_members.len()];
                    nearest(
                        &pool
                            .iter()
                            .copied()
                            .filter(|p| p % 12 == pc)
                            .collect::<Vec<_>>(),
                        i32::from(arrival) + 5,
                    )?
                } else {
                    arrival
                }
            }
            BassRole::Riff => {
                rule = "bass.riff";
                let pattern = [0, 0, 2, 0, 1, 0, 2, 1];
                let member = pattern[((pulse.start % (bar * 2)) / (PPQ / 2)) as usize % 8];
                let pc =
                    chord_members[(member + usize::from(bass.variation)) % chord_members.len()];
                nearest(
                    &pool
                        .iter()
                        .copied()
                        .filter(|p| p % 12 == pc)
                        .collect::<Vec<_>>(),
                    i32::from(arrival) + if beat % 2 == 0 { 0 } else { 12 },
                )?
            }
            BassRole::Walking => {
                if pulse.start == h.start {
                    arrival
                } else {
                    rule = "bass.walking-member";
                    let direction = if (phrase_bar + u32::from(bass.variation)) % 2 == 0 {
                        1
                    } else {
                        -1
                    };
                    nearest(&pool, i32::from(previous) + direction * 3)?
                }
            }
            BassRole::Melodic => {
                rule = "bass.melodic-contour";
                nearest(&pool, contour_target(c, spec, pulse.start))?
            }
        };
        if pulse.start == h.start && !matches!(bass.role, BassRole::Pedal | BassRole::Melodic) {
            pitch = arrival;
            rule = "bass.harmonic-arrival";
        }
        let next = pulses
            .get(i + 1)
            .and_then(|p| c.harmony_at(p.start))
            .or_else(|| {
                if c.looping {
                    spans.first().copied()
                } else {
                    None
                }
            });
        let mut goal = arrival;
        if let Some(next) = next
            && next.id != h.id
            && bass.role != BassRole::Pedal
            && bass.role != BassRole::Sub
        {
            goal = targets.get(&next.id).copied().unwrap_or(arrival);
            if bass.approaches == Decoration::Chromatic
                || bass.style == BassStyle::Jazz && bass.role == BassRole::Walking
            {
                let approach = i16::from(goal) + if bass.variation % 2 == 0 { -1 } else { 1 };
                if approach >= i16::from(spec.low) && approach <= i16::from(spec.high) {
                    pitch = approach as u8;
                    rule = "bass.chromatic-approach";
                }
            } else if bass.approaches == Decoration::Passing {
                let direction = if goal >= previous { -1 } else { 1 };
                if let Some(key) = c.key_at(pulse.start) {
                    let approach = key.step(goal, direction)?;
                    if approach >= spec.low && approach <= spec.high {
                        pitch = approach;
                        rule = "bass.step-approach";
                    }
                }
            } else if bass.approaches == Decoration::Anticipation {
                pitch = goal;
                rule = "bass.anticipation";
            }
        }
        if pitch < spec.low || pitch > spec.high {
            return Err(format!(
                "Bass pitch {pitch} is outside {}–{}",
                spec.low, spec.high
            ));
        }
        if !bass.tuning.is_empty()
            && !bass
                .tuning
                .iter()
                .any(|open| pitch >= *open && pitch <= open.saturating_add(24))
        {
            return Err(format!(
                "Bass pitch {pitch} has no position in the selected 24-fret tuning"
            ));
        }
        let mut provenance = Provenance::new(
            rule,
            Some(h.id),
            format!(
                "{} · {} · {}",
                bass.role.label(),
                bass.style.label(),
                bass.groove.label()
            ),
        );
        provenance.target = Some(goal);
        result.push(NoteEvent {
            id: identity(&[3, h.id, pulse.id, 0x62617373]),
            voice,
            member: None,
            pitch,
            start: pulse.start,
            length: pulse.length,
            velocity: pulse.velocity,
            provenance,
        });
    }
    if matches!(
        bass.approaches,
        Decoration::Neighbour | Decoration::Enclosure | Decoration::Suspension | Decoration::Escape
    ) {
        super::ornament::decorate(c, Voice::Bass, bass.approaches, &mut result)?;
    }
    for i in 0..result.len() {
        let next = if i + 1 < result.len() {
            Some(i + 1)
        } else if c.looping {
            Some(0)
        } else {
            None
        };
        if let Some(next) = next {
            let target = result[next].pitch;
            let n = &mut result[i];
            match n.provenance.rule.as_str() {
                "bass.chromatic-approach" => {
                    let pitch = i16::from(target) + if bass.variation % 2 == 0 { -1 } else { 1 };
                    if pitch < i16::from(spec.low) || pitch > i16::from(spec.high) {
                        return Err("Bass approach is outside its register".into());
                    }
                    n.pitch = pitch as u8;
                    n.provenance.target = Some(target);
                }
                "bass.step-approach" => {
                    n.pitch = c
                        .key_at(n.start)
                        .ok_or("Step approach needs a key")?
                        .step(target, if n.pitch < target { -1 } else { 1 })?;
                    n.provenance.target = Some(target);
                }
                "bass.anticipation" => {
                    n.pitch = target;
                    n.provenance.target = Some(target);
                }
                _ => {}
            }
        }
    }
    Ok(result)
}

pub(super) fn apply_overrides(
    c: &Composition,
    notes: &mut Vec<NoteEvent>,
    findings: &mut Vec<Finding>,
) -> Result<(), String> {
    for edit in &c.overrides {
        if edit
            .inserted
            .as_ref()
            .is_some_and(|n| !c.voices[n.voice.index()].enabled)
        {
            continue;
        }

        if edit.deleted {
            notes.retain(|n| n.id != edit.id);
            continue;
        }
        if let Some(inserted) = &edit.inserted {
            notes.retain(|n| n.id != edit.id);
            notes.push(inserted.clone());
        }
        if let Some(note) = notes.iter_mut().find(|n| n.id == edit.id) {
            if let Some(p) = edit.pitch {
                note.pitch = p;
            }
            if let Some(t) = edit.start {
                note.start = t;
            }
            if let Some(d) = edit.length {
                note.length = d;
            }
            if let Some(v) = edit.velocity {
                note.velocity = v;
            }
            if !note.provenance.detail.starts_with("Pinned · ") {
                note.provenance.detail = format!("Pinned · {}", note.provenance.detail);
            }
        } else {
            findings.push(Finding {
                rule: "override.orphan".into(),
                events: vec![edit.id],
                detail: "A locked source event no longer exists; the override has been retained"
                    .into(),
            });
        }
    }
    Ok(())
}

fn articulate(c: &Composition, notes: &mut [NoteEvent]) {
    let mut attacks: [Vec<u32>; 5] = std::array::from_fn(|_| Vec::new());
    for n in notes.iter() {
        attacks[n.voice.index()].push(n.start);
    }
    for times in &mut attacks {
        times.sort();
        times.dedup();
    }
    let bar = c.meter.bar().unwrap_or(192);
    let beat = PPQ * 4 / u32::from(c.meter.denominator);
    let mut groups = vec![0];
    for group in &c.meter.groups {
        groups.push(groups.last().copied().unwrap_or(0) + u32::from(*group) * beat);
    }
    for n in notes {
        let spec = &c.voices[n.voice.index()];
        let pos = (u64::from(n.start) * 1000 / u64::from(c.length)) as u16;
        let velocity_locked = c
            .overrides
            .iter()
            .any(|o| o.id == n.id && o.velocity.is_some());
        let length_locked = c
            .overrides
            .iter()
            .any(|o| o.id == n.id && o.length.is_some())
            || n.provenance
                .transforms
                .iter()
                .any(|t| t.starts_with("Tied through"));
        if !velocity_locked {
            if !spec.velocity_curve.is_empty() {
                n.velocity = (i32::from(n.velocity)
                    + (i32::from(curve(&spec.velocity_curve, pos)) - 50) / 2)
                    .clamp(1, 127) as u8;
            }
            if spec.articulation == Articulation::Accent {
                let accent = if n.start % (bar * u32::from(spec.melody.phrase_bars)) == 0 {
                    12
                } else if groups.contains(&(n.start % bar)) {
                    8
                } else if n.start % beat == 0 {
                    4
                } else {
                    -6
                };
                n.velocity = (i16::from(n.velocity) + accent).clamp(1, 127) as u8;
            }
        }
        if length_locked {
            continue;
        }
        let next = attacks[n.voice.index()].partition_point(|at| *at <= n.start);
        let end = attacks[n.voice.index()]
            .get(next)
            .copied()
            .unwrap_or(c.length);
        match spec.articulation {
            Articulation::Short => n.length = (n.length / 2).max(1),
            Articulation::Connected => n.length = end - n.start,
            Articulation::Legato => n.length = (end - n.start + 2).min(c.length - n.start),
            _ => {}
        }
        if !spec.gate_curve.is_empty() {
            n.length = (u64::from(n.length) * (50 + i64::from(curve(&spec.gate_curve, pos))) as u64
                / 100)
                .max(1)
                .min(u64::from(c.length - n.start)) as u32;
        }
    }
}

pub fn render(source: &Composition) -> Result<Rendered, String> {
    if source.engine != ENGINE_VERSION && !source.frozen && source.snapshot.is_some() {
        let mut frozen = source.clone();
        frozen.frozen = true;
        return render(&frozen);
    }
    source.validate()?;
    if source.frozen {
        let snapshot = source
            .snapshot
            .as_ref()
            .ok_or("Frozen recipe has no event snapshot")?;
        return Ok(Rendered {
            notes: snapshot.events.clone(),
            harmony: if snapshot.harmony.is_empty() {
                source.harmony.clone()
            } else {
                snapshot.harmony.clone()
            },
            voicings: snapshot.voicings.clone(),
            length: if snapshot.length == 0 {
                source.length
            } else {
                snapshot.length
            },
            fingerprint: source.fingerprint()?,
            ..Rendered::default()
        });
    }
    if !source.form.is_empty() {
        return super::form::render_form(source);
    }
    let c = super::form::expand(source)?;
    let mut result = Rendered {
        length: c.length,
        harmony: c.harmony.clone(),
        fingerprint: source.fingerprint()?,
        ..Rendered::default()
    };
    let fully_written = |voice| super::motif::covers_voice(&c, voice);
    let mut melody = if c.voices[2].enabled && !fully_written(Voice::Melody)? {
        melodic(&c, Voice::Melody)?
    } else {
        vec![]
    };
    let mut placements = Vec::new();
    let mut placed_count = 0;
    for p in &c.placements {
        let notes = motif::place(&c, p)?;
        placed_count += notes.len();
        if placed_count > MAX_EVENTS {
            return Err("Placements exceed the event budget".into());
        }
        placements.push(notes);
    }
    for (placement, events) in c.placements.iter().zip(&placements) {
        if placement.voice == Voice::Melody {
            let source = c
                .motifs
                .iter()
                .find(|m| m.id == placement.motif)
                .ok_or("Missing motif")?;
            let length =
                motif::transform(source, &placement.transforms, c.key_at(placement.start))?.length;
            melody.retain(|n| n.start < placement.start || n.start >= placement.start + length);
            melody.extend(events.clone());
        }
    }
    apply_overrides(&c, &mut melody, &mut Vec::new())?;
    // Only melody edits belong in this preliminary outer-voice snapshot.
    melody.retain(|n| n.voice == Voice::Melody);
    let mut bass = if c.voices[3].enabled && !fully_written(Voice::Bass)? {
        bassline(&c, &melody)?
    } else {
        vec![]
    };
    for (placement, events) in c.placements.iter().zip(&placements) {
        if placement.voice == Voice::Bass && c.voices[3].enabled {
            let source = c
                .motifs
                .iter()
                .find(|m| m.id == placement.motif)
                .ok_or("Missing motif")?;
            let length =
                motif::transform(source, &placement.transforms, c.key_at(placement.start))?.length;
            bass.retain(|n| n.start < placement.start || n.start >= placement.start + length);
            bass.extend(events.clone());
        }
    }
    apply_overrides(&c, &mut bass, &mut Vec::new())?;
    bass.retain(|n| n.voice == Voice::Bass);
    result.notes.extend(melody);
    result.notes.extend(bass);
    if c.voices[0].enabled && !fully_written(Voice::Chords)?
        || c.voices[1].enabled && !fully_written(Voice::Arp)?
    {
        result.voicings = voicing::solve(&c, &result.notes)?;
    }
    if c.voices[0].enabled && !fully_written(Voice::Chords)? {
        for pulse in rhythm::pulses(&c, Voice::Chords)? {
            let Some(h) = c.harmony_at(pulse.start) else {
                continue;
            };
            let Some(decision) = result.voicings.iter().find(|v| v.harmony == h.id) else {
                continue;
            };
            for &(member, pitch) in &decision.notes {
                if result.notes.len() >= MAX_EVENTS {
                    return Err("Composition exceeds the event budget".into());
                }
                if result.notes.len() >= MAX_EVENTS {
                    return Err("Composition exceeds the event budget".into());
                }
                result.notes.push(NoteEvent {
                    id: identity(&[0, h.id, pulse.id, member]),
                    voice: Voice::Chords,
                    member: Some(member),
                    pitch,
                    start: pulse.start,
                    length: pulse.length,
                    velocity: pulse.velocity,
                    provenance: Provenance::new(
                        "harmony.voicing",
                        Some(h.id),
                        format!(
                            "{} · cost {} · {} candidates · two-chord horizon",
                            h.voicing.layout.label(),
                            decision.cost.total(),
                            decision.candidates
                        ),
                    ),
                });
            }
        }
    }
    if c.voices[1].enabled && !fully_written(Voice::Arp)? {
        let spec = &c.voices[1];
        for (i, pulse) in rhythm::pulses(&c, Voice::Arp)?.iter().enumerate() {
            let Some(h) = c.harmony_at(pulse.start) else {
                continue;
            };
            let Some(decision) = result.voicings.iter().find(|v| v.harmony == h.id) else {
                continue;
            };
            let pitches = decision
                .notes
                .iter()
                .filter(|(_, p)| *p >= spec.low && *p <= spec.high)
                .copied()
                .collect::<Vec<_>>();
            if pitches.is_empty() {
                if h.material.members.is_empty() {
                    continue;
                }
                return Err("Arpeggio register contains no voiced members".into());
            }
            let n = pitches.len();
            let at = i + usize::from(spec.melody.variation);
            let index = match spec.melody.contour {
                Contour::Fall => n - 1 - at % n,
                Contour::Waves | Contour::Arch => {
                    let cycle = at % (2 * n - 1);
                    if cycle < n { cycle } else { 2 * n - 2 - cycle }
                }
                _ => at % n,
            };
            let (member, pitch) = pitches[index];
            if result.notes.len() >= MAX_EVENTS {
                return Err("Composition exceeds the event budget".into());
            }
            result.notes.push(NoteEvent {
                id: identity(&[1, h.id, pulse.id, member]),
                voice: Voice::Arp,
                member: Some(member),
                pitch,
                start: pulse.start,
                length: pulse.length,
                velocity: pulse.velocity,
                provenance: Provenance::new(
                    "arpeggio.ordered-members",
                    Some(h.id),
                    format!(
                        "{} · member {} of {n}",
                        spec.melody.contour.label(),
                        index + 1
                    ),
                ),
            });
        }
    }
    for (placement, events) in c.placements.iter().zip(placements) {
        if matches!(placement.voice, Voice::Melody | Voice::Bass)
            || !c.voices[placement.voice.index()].enabled
        {
            continue;
        }
        let motif = c
            .motifs
            .iter()
            .find(|m| m.id == placement.motif)
            .ok_or("Missing motif")?;
        let length =
            motif::transform(motif, &placement.transforms, c.key_at(placement.start))?.length;
        result.notes.retain(|n| {
            n.voice != placement.voice
                || n.start < placement.start
                || n.start >= placement.start + length
        });
        result.notes.extend(events);
    }
    if c.voices[4].enabled && !fully_written(Voice::Counterpoint)? {
        let (notes, findings) = super::counterpoint::compose(&c, &result.notes)?;
        result.notes.extend(notes);
        result.findings.extend(findings);
    }
    result.length = result
        .notes
        .iter()
        .map(NoteEvent::end)
        .max()
        .unwrap_or(c.length)
        .max(c.length);
    let mut c = c;
    c.length = result.length;
    validate_events(&c, &result.notes, &mut Vec::new())?;
    super::rhythm::tie(&c, &mut result.notes)?;
    articulate(&c, &mut result.notes);
    apply_overrides(&c, &mut result.notes, &mut result.findings)?;
    super::form::arrange(&c, &mut result.notes)?;
    result
        .notes
        .sort_by_key(|n| (n.start, n.voice.index(), n.pitch, n.id));
    validate_events(&c, &result.notes, &mut result.findings)?;
    super::ornament::validate(&c, &result.notes)?;
    super::counterpoint::validate(&c, &result.notes, &mut result.findings)?;
    if result.notes.len() > MAX_EVENTS {
        return Err("Composition exceeds 32768 events".into());
    }
    Ok(result)
}

pub fn validate_events(
    c: &Composition,
    events: &[NoteEvent],
    findings: &mut Vec<Finding>,
) -> Result<(), String> {
    let mut ids = std::collections::BTreeSet::new();
    for n in events {
        if !ids.insert(n.id) {
            return Err("Two events share an identity".into());
        }
        if n.pitch > 127
            || n.velocity == 0
            || n.velocity > 127
            || n.length == 0
            || n.start
                .checked_add(n.length)
                .is_none_or(|end| end > c.length)
        {
            return Err("Event exceeds MIDI or timeline limits".into());
        }
        let manual =
            c.overrides.iter().any(|o| o.id == n.id && !o.deleted) || n.provenance.motif.is_some();
        let spec = &c.voices[n.voice.index()];
        if !manual && (n.pitch < spec.low || n.pitch > spec.high) {
            return Err("Generated event exceeds the voice's hard range".into());
        }
        for h in c
            .harmony
            .iter()
            .filter(|h| h.start < n.end() && h.start + h.length > n.start)
        {
            if !manual
                && let Some(profile) = spec.profile
                && let Ok(context) = crate::theory::harmony::context(&h.material.source)
            {
                if !crate::theory::harmony::allows(&context, n.pitch, profile) {
                    return Err(format!(
                        "MIDI {} at tick {} is excluded by the selected {} profile over {}",
                        n.pitch,
                        n.start,
                        profile.label(),
                        h.material.label()
                    ));
                }
            }
            let written_bass = n.voice == Voice::Bass
                && (h.material.bass.is_some_and(|b| match b {
                    Bass::Pitch(p) => p == n.pitch,
                    Bass::Class(p) => p == n.pitch % 12,
                }) || h.material.root == Some(n.pitch % 12));
            if h.material.mask() & (1 << (n.pitch % 12)) == 0 && !written_bass {
                let named = matches!(
                    n.provenance.rule.as_str(),
                    "bass.neighbour"
                        | "bass.enclosure-upper"
                        | "bass.enclosure-lower"
                        | "bass.escape"
                        | "bass.suspension"
                        | "bass.pedal"
                        | "bass.chromatic-approach"
                        | "bass.step-approach"
                        | "bass.anticipation"
                        | "melody.chromatic-approach"
                        | "melody.anticipation"
                        | "melody.suspension"
                        | "melody.passing"
                        | "melody.neighbour"
                        | "melody.enclosure"
                        | "melody.escape"
                        | "melody.phrase-arrival"
                        | "counterpoint.canon"
                        | "counterpoint.species"
                );
                if !named && !manual {
                    findings.push(Finding {
                        rule: "harmony.crossed-span".into(),
                        events: vec![n.id],
                        detail: format!(
                            "Pitch {} continues outside {} across this span",
                            n.pitch,
                            h.material.label()
                        ),
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_is_identical_and_each_note_has_a_rule() {
        let mut c = Composition::default();
        c.voices[2].enabled = true;
        c.voices[3].enabled = true;
        let a = render(&c).unwrap();
        let b = render(&c).unwrap();
        assert_eq!(a, b);
        assert!(a.notes.iter().all(|n| !n.provenance.rule.is_empty()));
    }
    #[test]
    fn pedal_survives_chromatic_harmony() {
        let mut c = Composition::default();
        c.voices[0].enabled = false;
        c.voices[3].enabled = true;
        c.voices[3].bass.role = BassRole::Pedal;
        c.voices[3].bass.pedal = 36;
        let r = render(&c).unwrap();
        assert!(r.notes.iter().all(|n| n.pitch == 36));
        assert!(r.notes.iter().any(|n| n.start >= 576));
    }
    #[test]
    fn pins_are_exact_and_deletions_survive_regeneration() {
        let mut c = Composition::default();
        c.voices[2].enabled = true;
        let before = render(&c).unwrap();
        let mut note = before
            .notes
            .iter()
            .find(|n| n.voice == Voice::Melody)
            .unwrap()
            .clone();
        note.pitch = 61;
        c.pin(&note);
        c.voices[2].melody.variation = 4;
        let next = render(&c).unwrap();
        assert_eq!(
            next.notes.iter().find(|n| n.id == note.id).unwrap().pitch,
            61
        );
        c.remove_note(note.id);
        assert!(!render(&c).unwrap().notes.iter().any(|n| n.id == note.id));
    }
}
