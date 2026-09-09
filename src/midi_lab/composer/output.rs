use super::*;
use crate::{
    midi_lab::{Destination, Recipe, Voice},
    sequencing::{Note, PATTERN_STEP_TICKS, Pattern, Song},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Delivery {
    pub destination: Destination,
    pub events: Vec<NoteEvent>,
    pub changes: Vec<String>,
}

pub fn deliveries(
    c: &Composition,
    rendered: &Rendered,
    primary: Destination,
) -> Result<Vec<Delivery>, String> {
    let mut groups: BTreeMap<(u64, u64), Delivery> = BTreeMap::new();
    for voice in Voice::ALL {
        if c.voices[voice.index()].enabled {
            let destination = c.destinations[voice.index()].unwrap_or(primary);
            groups
                .entry((destination.track.0, destination.pattern.0))
                .or_insert_with(|| Delivery {
                    destination,
                    events: vec![],
                    changes: vec![],
                });
        }
    }
    for event in &rendered.notes {
        let destination = c
            .event_destinations
            .get(&event.id)
            .or_else(|| {
                event
                    .provenance
                    .origin
                    .and_then(|id| c.event_destinations.get(&id))
            })
            .copied()
            .or(c.destinations[event.voice.index()])
            .unwrap_or(primary);
        groups
            .entry((destination.track.0, destination.pattern.0))
            .or_insert_with(|| Delivery {
                destination,
                events: vec![],
                changes: vec![],
            })
            .events
            .push(event.clone());
    }
    for group in groups.values_mut() {
        group
            .events
            .sort_by_key(|n| (n.start, n.pitch, n.voice.index(), n.id));
        let mut merged: Vec<NoteEvent> = Vec::new();
        let mut active: [Option<usize>; 128] = [None; 128];
        for event in &group.events {
            if event.pitch > 127 {
                return Err("Invalid MIDI pitch".into());
            }
            if let Some(previous) = active[event.pitch as usize]
                .and_then(|i| merged.get_mut(i))
                .filter(|n| n.end() > event.start)
            {
                if c.output == OutputPolicy::Merge && previous.start == event.start {
                    group.changes.push(format!(
                        "Merge {} and {} on pitch {} at tick {}: lengths {}/{} → {}, velocities {}/{} → {}",
                        previous.voice.label(),
                        event.voice.label(),
                        event.pitch,
                        event.start,
                        previous.length,event.length,previous.length.max(event.length),
                        previous.velocity,event.velocity,previous.velocity.max(event.velocity)
                    ));
                    previous.length = previous.length.max(event.length);
                    previous.velocity = previous.velocity.max(event.velocity);
                    continue;
                }
                return Err(format!(
                    "{} and {} overlap on pitch {} at tick {}. Choose separate voice destinations{}",
                    previous.voice.label(),
                    event.voice.label(),
                    event.pitch,
                    event.start,
                    if previous.start == event.start {
                        " or explicitly select coincident-note merging."
                    } else {
                        " or edit their releases."
                    }
                ));
            }
            active[event.pitch as usize] = Some(merged.len());
            merged.push(event.clone());
        }
        group.events = merged;
    }
    // Two clips on the same track still share one note-addressed instrument.
    let mut tracks: BTreeMap<u64, Vec<(&NoteEvent, u64)>> = BTreeMap::new();
    for d in groups.values() {
        for n in &d.events {
            tracks
                .entry(d.destination.track.0)
                .or_default()
                .push((n, d.destination.pattern.0));
        }
    }
    for events in tracks.values_mut() {
        events.sort_by_key(|(n, _)| (n.start, n.id));
        let mut active: [Option<(u32, u64)>; 128] = [None; 128];
        for (n, clip) in events {
            if active[n.pitch as usize].is_some_and(|(end, prior)| end > n.start && prior != *clip)
            {
                return Err("Overlapping parts need separate instrument tracks".into());
            }
            active[n.pitch as usize] = Some((n.end(), *clip));
        }
    }
    Ok(groups.into_values().collect())
}

pub fn write(
    pattern: &mut Pattern,
    recipe: &Recipe,
    notes: &[NoteEvent],
    length: u32,
) -> Result<(), String> {
    let mut next = Pattern::empty(pattern.id, pattern.name.clone());
    next.tag = pattern.tag.clone();
    next.extend_timeline(length as usize)?;
    next.midi_lab = Some(recipe.clone());
    for n in notes {
        if n.start >= length || n.end() > length {
            return Err("Output note exceeds the delivered timeline".into());
        }
        let mut note = Note::new(n.pitch, n.length as usize, n.velocity);
        note.micro_ticks = (n.start as usize % PATTERN_STEP_TICKS) as i16;
        next.trig_mut(n.start as usize / PATTERN_STEP_TICKS)
            .add_tone_at(note);
    }
    *pattern = next;
    Ok(())
}

pub fn apply(
    song: &mut Song,
    recipe: &Recipe,
    primary: Destination,
    rendered: &Rendered,
) -> Result<Vec<Delivery>, String> {
    let c = recipe
        .composition
        .as_ref()
        .ok_or("Missing composition document")?;
    let deliveries = deliveries(c, rendered, primary)?;
    // Validate every target before the first mutation, making failure atomic.
    let mut patterns = Vec::new();
    for delivery in &deliveries {
        let track = song
            .tracks
            .iter()
            .find(|t| t.id == delivery.destination.track && t.machine.is_some())
            .ok_or("Destination instrument was removed")?;
        let mut pattern = song
            .pattern(delivery.destination.pattern)
            .ok_or("Destination clip was removed")?
            .clone();
        if !pattern
            .tag
            .strip_prefix(&track.letter)
            .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
        {
            return Err("Clip does not belong to the selected destination track".into());
        }
        write(&mut pattern, recipe, &delivery.events, rendered.length)?;
        patterns.push(pattern);
    }
    let mut next = song.clone();
    let start = next
        .tracks
        .iter()
        .find(|t| t.id == primary.track)
        .and_then(|t| t.blocks.iter().find(|b| b.pattern_id == primary.pattern))
        .map_or(0, |b| b.start_tick);
    for pattern in patterns {
        let id = pattern.id;
        *next.pattern_mut(id).ok_or("Destination clip was removed")? = pattern;
    }
    for delivery in &deliveries {
        let at = next
            .tracks
            .iter()
            .position(|t| t.id == delivery.destination.track)
            .ok_or("Destination track was removed")?;
        let blocks = next.tracks[at]
            .blocks
            .iter()
            .filter(|b| b.pattern_id == delivery.destination.pattern)
            .map(|b| (b.id, b.length_ticks))
            .collect::<Vec<_>>();
        if blocks.is_empty() {
            next.place_block(at,delivery.destination.pattern,start,rendered.length as usize).map_err(|_|"The complete output span overlaps an unrelated arrangement block; choose a clear destination")?;
        } else {
            for (id, length) in blocks {
                if length < rendered.length as usize
                    && !next.resize_block(at, id, rendered.length as usize)
                {
                    return Err("The longer output would overlap another arrangement block; move that block or choose a clear destination".into());
                }
            }
        }
    }
    *song = next;
    Ok(deliveries)
}

pub fn audition(
    mut song: Song,
    recipe: &Recipe,
    primary: Destination,
    rendered: &Rendered,
) -> Result<Song, String> {
    let (bpm, tempo) = tempo_context(&song, primary);
    song.bpm = bpm;
    song.tempo = tempo;
    let drums = recipe.composition.as_ref().and_then(|c| {
        let b = &c.voices[Voice::Bass.index()].bass;
        if b.audition_drums {
            b.kick_source
        } else {
            None
        }
    });
    let deliveries = apply(&mut song, recipe, primary, rendered)?;
    for track in &mut song.tracks {
        track.blocks.clear();
        track.audio_blocks.clear();
        track.automation.clear();
        track.solo = false;
        track.muted = false;
        track.armed = false;
        track.monitor = crate::sequencing::Monitor::Off;
    }
    // Preserve the composition's current tempo context in the snapshot.
    song.loop_on = false;
    if let Some(d) = drums {
        if deliveries.iter().any(|g| g.destination.track == d.track) {
            return Err("Drum audition needs a track separate from the generated voices".into());
        }
        let at = song
            .tracks
            .iter()
            .position(|t| t.id == d.track)
            .ok_or("Drum source track was removed")?;
        if song.pattern(d.pattern).is_none() {
            return Err("Drum source clip was removed".into());
        }
        song.place_block(at, d.pattern, 0, rendered.length as usize)
            .map_err(|_| "Cannot place the drum audition source")?;
    }
    for delivery in deliveries {
        let track = song
            .tracks
            .iter()
            .position(|t| t.id == delivery.destination.track)
            .ok_or("Missing preview track")?;
        song.place_block(
            track,
            delivery.destination.pattern,
            0,
            rendered.length as usize,
        )
        .map_err(|e| format!("Cannot place preview: {e:?}"))?;
    }
    Ok(song)
}

fn variable(mut value: u32, bytes: &mut Vec<u8>) {
    let mut buffer = [0u8; 4];
    let mut at = 3;
    buffer[at] = (value & 0x7f) as u8;
    while {
        value >>= 7;
        value != 0
    } {
        at -= 1;
        buffer[at] = ((value & 0x7f) as u8) | 0x80;
    }
    bytes.extend_from_slice(&buffer[at..]);
}
fn chunk(tag: &[u8; 4], data: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
}

/// SMF type 1, separate voice tracks/channels, exact 48 PPQ musical events.
/// Coincident duplicate pitches within one part require explicit conversion.
pub fn midi(rendered: &Rendered, bpm: u16) -> Result<Vec<u8>, String> {
    midi_with_context(rendered, f64::from(bpm), &[], &Meter::default())
}

pub fn midi_with_context(
    rendered: &Rendered,
    bpm: f64,
    marks: &[crate::sequencing::TempoMark],
    meter: &Meter,
) -> Result<Vec<u8>, String> {
    meter.bar()?;
    if rendered.length > MAX_TICKS
        || rendered.notes.len() > MAX_EVENTS
        || rendered.notes.iter().any(|n| {
            n.pitch > 127
                || n.velocity == 0
                || n.velocity > 127
                || n.length == 0
                || n.end() > rendered.length
        })
    {
        return Err("Invalid event snapshot for MIDI export".into());
    }
    let encode_tempo = |bpm: f64| -> Result<[u8; 3], String> {
        if !bpm.is_finite() || !(4.0..=1000.0).contains(&bpm) {
            return Err("SMF tempo must be 4–1000 BPM".into());
        }
        let n = (60_000_000. / bpm).round() as u32;
        Ok([
            ((n >> 16) & 255) as u8,
            ((n >> 8) & 255) as u8,
            (n & 255) as u8,
        ])
    };
    let mut tracks = Vec::new();
    for voice in Voice::ALL {
        let notes = rendered
            .notes
            .iter()
            .filter(|n| n.voice == voice)
            .cloned()
            .collect::<Vec<_>>();
        for (i, lane) in lanes(&notes).into_iter().enumerate() {
            tracks.push((
                format!(
                    "{}{}",
                    voice.label(),
                    if i == 0 {
                        String::new()
                    } else {
                        format!(" {}", i + 1)
                    }
                ),
                lane,
            ));
        }
    }
    if tracks.len() > 15 {
        return Err("SMF export needs more than 15 independent melodic channels; split the composition across files or reduce overlapping unisons explicitly".into());
    }
    let mut out = Vec::new();
    let mut header = Vec::new();
    header.extend_from_slice(&1u16.to_be_bytes());
    header.extend_from_slice(&((tracks.len() + 1) as u16).to_be_bytes());
    header.extend_from_slice(&(PPQ as u16).to_be_bytes());
    chunk(b"MThd", &header, &mut out);
    let mut conductor = vec![
        0,
        0xff,
        0x58,
        4,
        meter.numerator,
        meter.denominator.ilog2() as u8,
        24,
        8,
    ];
    let mut tempo_events = std::collections::BTreeMap::new();
    tempo_events.insert(0, encode_tempo(bpm)?);
    for mark in marks.iter().filter(|m| m.tick < rendered.length as usize) {
        tempo_events.insert(mark.tick as u32, encode_tempo(mark.bpm)?);
    }
    let mut last = 0;
    for (tick, tempo) in tempo_events {
        variable(tick - last, &mut conductor);
        conductor.extend_from_slice(&[0xff, 0x51, 3]);
        conductor.extend_from_slice(&tempo);
        last = tick;
    }
    variable(rendered.length - last, &mut conductor);
    conductor.extend_from_slice(&[0xff, 0x2f, 0]);
    chunk(b"MTrk", &conductor, &mut out);
    for (index, (name, notes)) in tracks.into_iter().enumerate() {
        let channel = if index >= 9 { index + 1 } else { index } as u8;
        let mut events = Vec::new();
        for note in notes {
            events.push((note.start, 1u8, note.pitch, note.velocity));
            events.push((note.end(), 0u8, note.pitch, 0));
        }
        events.sort();
        let mut track = vec![0, 0xff, 3];
        variable(name.len() as u32, &mut track);
        track.extend_from_slice(name.as_bytes());
        let mut previous = 0;
        for (tick, kind, pitch, velocity) in events {
            variable(tick - previous, &mut track);
            track.extend_from_slice(&[
                if kind == 0 {
                    0x80 | channel
                } else {
                    0x90 | channel
                },
                pitch,
                velocity,
            ]);
            previous = tick;
        }
        variable(rendered.length.saturating_sub(previous), &mut track);
        track.extend_from_slice(&[0xff, 0x2f, 0]);
        chunk(b"MTrk", &track, &mut out);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_output_preserves_events_after_the_old_grid_boundary() {
        let mut c = Composition::default();
        c.progression("C:16 Dm:16").unwrap();
        c.voices[0].rhythm.kind = RhythmKind::Quarter;
        let rendered = render(&c).unwrap();
        let recipe = Recipe {
            composition: Some(Box::new(c)),
            ..Recipe::default()
        };
        let mut pattern = Pattern::default();
        write(&mut pattern, &recipe, &rendered.notes, rendered.length).unwrap();
        assert_eq!(pattern.length_ticks, 1536);
        assert_eq!(pattern.step_count(), 128);
        assert!(!pattern.trig(96).notes.is_empty());
        let last = &rendered.notes[rendered.notes.len() - 1];
        assert!(
            pattern
                .trig(last.start as usize / 12)
                .notes
                .iter()
                .any(|n| n.length_ticks == last.length as usize)
        );
    }
    #[test]
    fn collisions_refuse_without_mutating_the_song() {
        let mut c = Composition::default();
        c.harmony[0].material = crate::theory::material::Material::parse("notes:C4,C4").unwrap();
        let rendered = render(&c).unwrap();
        let recipe = Recipe {
            composition: Some(Box::new(c)),
            ..Recipe::default()
        };
        let mut song = Song::default();
        let before = song.clone();
        let dest = Destination {
            track: song.tracks[0].id,
            pattern: song.patterns[0].id,
        };
        assert!(apply(&mut song, &recipe, dest, &rendered).is_err());
        assert_eq!(song, before);
    }
    #[test]
    fn midi_has_header_tempo_voice_tracks_and_note_offs() {
        let c = Composition::default();
        let r = render(&c).unwrap();
        let bytes = midi(&r, 120).unwrap();
        assert_eq!(&bytes[..4], b"MThd");
        assert_eq!(&bytes[8..14], &[0, 1, 0, 2, 0, 48]);
        assert!(
            bytes
                .windows(6)
                .any(|w| w == [0xff, 0x51, 3, 7, 0xa1, 0x20])
        );
        assert!(bytes.windows(3).any(|w| w[0] == 0x80 && w[2] == 0));
    }
}

/// Minimum independent lanes for note-addressed output. Different pitches may
/// remain polyphonic; overlapping instances of one pitch get separate lanes.
pub fn lanes(events: &[NoteEvent]) -> Vec<Vec<NoteEvent>> {
    let mut events = events.to_vec();
    events.sort_by_key(|n| (n.start, n.pitch, n.id));
    let mut lanes: Vec<Vec<NoteEvent>> = Vec::new();
    let mut releases: Vec<[u32; 128]> = Vec::new();
    for n in events {
        if n.pitch > 127 {
            continue;
        }
        let pitch = n.pitch as usize;
        let lane = releases
            .iter()
            .position(|end| end[pitch] <= n.start)
            .unwrap_or(lanes.len());
        if lane == lanes.len() {
            lanes.push(vec![]);
            releases.push([0; 128]);
        }
        releases[lane][pitch] = n.end();
        lanes[lane].push(n);
    }
    lanes
}

/// Audition and export start at the primary arrangement placement's tempo.
pub fn tempo_context(
    song: &Song,
    primary: Destination,
) -> (f64, Vec<crate::sequencing::TempoMark>) {
    let start = song
        .tracks
        .iter()
        .find(|t| t.id == primary.track)
        .and_then(|t| t.blocks.iter().find(|b| b.pattern_id == primary.pattern))
        .map_or(0, |b| b.start_tick);
    let bpm = song
        .tempo
        .iter()
        .filter(|m| m.tick <= start)
        .max_by_key(|m| m.tick)
        .map_or(song.bpm, |m| m.bpm);
    let mut marks = song
        .tempo
        .iter()
        .filter(|m| m.tick > start)
        .map(|m| crate::sequencing::TempoMark {
            tick: m.tick - start,
            bpm: m.bpm,
        })
        .collect::<Vec<_>>();
    marks.sort_by_key(|m| m.tick);
    (bpm, marks)
}

pub fn tick_at_seconds(bpm: f64, marks: &[crate::sequencing::TempoMark], mut seconds: f64) -> f64 {
    let mut at = 0.;
    let mut bpm = bpm;
    for m in marks {
        let duration = (m.tick as f64 - at) / 48. * 60. / bpm;
        if seconds < duration {
            return at + seconds * bpm / 60. * 48.;
        }
        seconds -= duration;
        at = m.tick as f64;
        bpm = m.bpm;
    }
    at + seconds * bpm / 60. * 48.
}
