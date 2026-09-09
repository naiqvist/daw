use super::*;
use crate::theory::{
    self,
    harmony::{self, Tone},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Generated {
    pub notes: Vec<Event>,
    pub voicings: Vec<(u64, Vec<Tone>)>,
    pub warnings: Vec<String>,
}
fn hash(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
fn identity(voice: Voice, h: u64, gate: u64, tone: usize) -> u64 {
    hash(
        (voice.index() as u64 + 1) * 7919
            ^ h.wrapping_mul(65537)
            ^ gate.wrapping_mul(257)
            ^ tone as u64,
    )
}

pub fn gates(recipe: &Recipe, voice: Voice) -> Vec<Gate> {
    let v = &recipe.voices[voice.index()];
    if v.rhythm == Rhythm::Custom {
        let mut gates = v.custom.clone();
        gates.sort_by_key(|g| g.start);
        return gates;
    }
    if v.rhythm == Rhythm::Hold {
        return recipe
            .harmony
            .iter()
            .map(|h| Gate {
                id: h.id,
                start: h.start,
                length: if voice == Voice::Chords {
                    h.length
                } else {
                    h.length * usize::from(v.gate) / 100
                }
                .max(1),
                velocity: v.velocity,
            })
            .collect();
    }
    let step = match v.rhythm {
        Rhythm::Quarter => 48,
        Rhythm::Eighth => 24,
        Rhythm::Triplet => 16,
        _ => 12,
    };
    let mut result = Vec::new();
    for (i, t) in (0..recipe.length).step_by(step).enumerate() {
        let hit = match v.rhythm {
            Rhythm::Syncopated => [0, 3, 6, 10, 12, 15].contains(&(i % 16)),
            Rhythm::Euclidean => {
                (((i + v.rotation as usize) % v.steps.clamp(1, 32) as usize)
                    * v.pulses.min(v.steps) as usize
                    % v.steps.clamp(1, 32) as usize)
                    < v.pulses.min(v.steps) as usize
            }
            _ => true,
        };
        if !hit {
            continue;
        }
        let swing = if i % 2 == 1 && v.rhythm != Rhythm::Triplet {
            step * (v.swing.clamp(50, 75) as usize - 50) / 50
        } else {
            0
        };
        let start = t + swing;
        if start < recipe.length {
            result.push(Gate {
                id: i as u64,
                start,
                length: (step * v.gate.clamp(5, 100) as usize / 100)
                    .max(1)
                    .min(recipe.length - start),
                velocity: v.velocity,
            });
        }
    }
    result
}

pub fn generate(recipe: &Recipe) -> Result<Generated, String> {
    if let Some(composition) = &recipe.composition {
        return super::composer::bridge::generated(composition);
    }
    if recipe.length == 0 || recipe.length > DEFAULT_PATTERN_TICKS {
        return Err("Clip length must be 1–16 beats".into());
    }
    if recipe.harmony.len() > 64
        || recipe.pinned.len() > 2048
        || recipe.voices.iter().any(|v| v.custom.len() > 256)
    {
        return Err("Clip exceeds the event limit".into());
    }
    let mut harmony = recipe.harmony.iter().collect::<Vec<_>>();
    harmony.sort_by_key(|h| h.start);
    for (i, h) in harmony.iter().enumerate() {
        if h.length == 0 || h.start >= recipe.length || h.length > recipe.length - h.start {
            return Err("A harmony span is outside the clip".into());
        }
        if i > 0 && harmony[i - 1].start + harmony[i - 1].length > h.start {
            return Err("Harmony spans overlap".into());
        }
    }
    let mut result = Generated::default();
    let mut previous = vec![];
    for h in harmony {
        let tones = harmony::realise(&h.symbol, &h.voicing, recipe.style, &previous)
            .map_err(|e| format!("{}: {e}", h.symbol))?;
        previous = tones.clone();
        result.voicings.push((h.id, tones));
    }
    for voice in Voice::ALL {
        let spec = &recipe.voices[voice.index()];
        if !spec.enabled {
            continue;
        }
        if spec.low > spec.high || spec.high > 127 {
            return Err(format!("{} range is reversed", voice.label()));
        }
        let mut prior = None;
        let mut prior_ref = None;
        for (index, g) in gates(recipe, voice).iter().enumerate() {
            let Some(h) = recipe.harmony_at(g.start) else {
                continue;
            };
            let chord = harmony::context(&h.symbol)?;
            let tones = &result
                .voicings
                .iter()
                .find(|(id, _)| *id == h.id)
                .ok_or("Missing voicing")?
                .1;
            let pool = (spec.low..=spec.high)
                .filter(|&p| harmony::allows(&chord, p, recipe.style))
                .collect::<Vec<_>>();
            if pool.is_empty() {
                return Err(format!(
                    "{} has no compatible notes in its range",
                    voice.label()
                ));
            }
            let r = hash(spec.seed ^ h.id.wrapping_mul(31) ^ index as u64);
            let reference = result
                .notes
                .iter()
                .chain(recipe.pinned.iter())
                .filter(|n| {
                    recipe.voices[Voice::Melody.index()].enabled
                        && n.voice == Voice::Melody
                        && n.start <= g.start
                        && n.start + n.length > g.start
                })
                .max_by_key(|n| n.start)
                .map(|n| (n.pitch, n.start + n.length));
            let reference_end = result
                .notes
                .iter()
                .chain(recipe.pinned.iter())
                .filter(|n| n.voice == Voice::Melody && n.start > g.start)
                .map(|n| n.start)
                .min()
                .unwrap_or(recipe.length)
                .min(reference.map_or(recipe.length, |n| n.1));
            let reference = reference.map(|n| n.0);
            let pitches = match voice {
                Voice::Chords => tones.iter().map(|n| n.pitch).collect(),
                Voice::Arp => {
                    let candidates = tones
                        .iter()
                        .filter(|n| n.pitch >= spec.low && n.pitch <= spec.high)
                        .map(|n| n.pitch)
                        .collect::<Vec<_>>();
                    if candidates.is_empty() {
                        return Err("Arpeggio range contains no voiced chord notes".into());
                    }
                    let n = candidates.len();
                    let index = index + (spec.seed.saturating_sub(1) as usize % n);
                    let i = match spec.motion {
                        1 => n - 1 - index % n,
                        2 => {
                            let t = index % (2 * n - 1);
                            if t < n { t } else { 2 * n - 2 - t }
                        }
                        3 => r as usize % n,
                        _ => index % n,
                    };
                    vec![candidates[i]]
                }
                Voice::Bass => {
                    let target = match spec.motion {
                        1 if index % 4 != 0 => {
                            prior.unwrap_or(36) + if index % 2 == 0 { 2 } else { -2 }
                        }
                        2 => 36,
                        3 => 36 + i16::from(chord.root_pc) + [0, 7, 12][r as usize % 3],
                        _ => 36 + i16::from(chord.root_pc),
                    };
                    let roots = pool
                        .iter()
                        .copied()
                        .filter(|p| match spec.motion {
                            1 => true,
                            2 => *p % 12 == 0,
                            3 => [chord.root_pc, (chord.root_pc + 7) % 12].contains(&(*p % 12)),
                            _ => *p % 12 == chord.root_pc,
                        })
                        .collect::<Vec<_>>();
                    if spec.motion == 2 && roots.is_empty() {
                        result
                            .warnings
                            .push("C pedal rests over chords that exclude C".into());
                        continue;
                    }
                    vec![nearest(
                        if roots.is_empty() { &pool } else { &roots },
                        target,
                    )]
                }
                Voice::Melody => {
                    let center = (i16::from(spec.low) + i16::from(spec.high)) / 2;
                    let motif = [0, 2, 4, 2, 5, 4, 2, -1];
                    let delta = match spec.motion {
                        1 => [-7, 5, 9, -4][index % 4],
                        2 => motif[index % 8],
                        3 => (r % 13) as i16 - 6,
                        _ => [-2, 2, 1, -1, 3, -2, 0, 2][(r as usize + index) % 8],
                    };
                    let target = if spec.motion == 2 {
                        center + delta
                    } else {
                        prior.unwrap_or(center) + delta
                    };
                    let stable = pool
                        .iter()
                        .copied()
                        .filter(|p| {
                            chord.members.iter().any(|m| {
                                (i16::from(*p) - i16::from(chord.root_pc)).rem_euclid(12)
                                    == m.semitones.rem_euclid(12)
                            })
                        })
                        .collect::<Vec<_>>();
                    vec![nearest(
                        if g.start % 48 == 0 && !stable.is_empty() {
                            &stable
                        } else {
                            &pool
                        },
                        target,
                    )]
                }
                Voice::Counterpoint => {
                    let Some(reference) = reference else {
                        result
                            .warnings
                            .push("Counterpoint needs a sounding Melody voice".into());
                        continue;
                    };
                    let target = prior.unwrap_or(i16::from(reference) - 7) + (r % 5) as i16 - 2
                        + match prior_ref {
                            Some(p) if reference > p => -2,
                            Some(p) if reference < p => 2,
                            _ => 0,
                        };
                    let candidates = pool
                        .iter()
                        .copied()
                        .filter(|&p| {
                            p < reference
                                && theory::is_consonant(p, reference)
                                && match (prior, prior_ref) {
                                    (Some(a), Some(b)) => {
                                        !theory::is_parallel_perfect((a as u8, b), (p, reference))
                                    }
                                    _ => true,
                                }
                        })
                        .collect::<Vec<_>>();
                    prior_ref = Some(reference);
                    if candidates.is_empty() {
                        result.warnings.push(
                            "Counterpoint rests where no consonant, nonparallel note fits".into(),
                        );
                        continue;
                    }
                    vec![nearest(&candidates, target)]
                }
            };
            for (i, pitch) in pitches.into_iter().enumerate() {
                let id = identity(voice, h.id, g.id, i);
                if !recipe.removed.contains(&id) && !recipe.pinned.iter().any(|n| n.id == id) {
                    result.notes.push(Event {
                        id,
                        voice,
                        pitch,
                        start: g.start,
                        length: g.length.max(1).min(h.start + h.length - g.start).min(
                            if voice == Voice::Counterpoint {
                                reference_end - g.start
                            } else {
                                recipe.length
                            },
                        ),
                        velocity: g.velocity.clamp(1, 127),
                    });
                }
                prior = Some(i16::from(pitch));
            }
        }
    }
    for n in &recipe.pinned {
        if !recipe.voices[n.voice.index()].enabled {
            continue;
        }
        let Some(h) = recipe.harmony_at(n.start) else {
            return Err("A pinned note lies outside harmony; move or remove it".into());
        };
        if !harmony::allows(&harmony::context(&h.symbol)?, n.pitch, recipe.style) {
            return Err(format!(
                "Pinned note {} conflicts with {}; move, unpin or use Chromatic",
                n.pitch, h.symbol
            ));
        }
        if n.pitch > 127
            || n.velocity == 0
            || n.velocity > 127
            || n.length == 0
            || n.start >= recipe.length
            || n.length > recipe.length - n.start
        {
            return Err("A pinned note lies outside the clip".into());
        }
        // Check every harmony crossed by a held, manually edited note.
        for next in &recipe.harmony {
            if next.start > n.start
                && next.start < n.start + n.length
                && !harmony::allows(&harmony::context(&next.symbol)?, n.pitch, recipe.style)
            {
                return Err("A pinned note crosses into an incompatible chord".into());
            }
        }
        result.notes.push(n.clone());
    }
    let pinned = recipe
        .pinned
        .iter()
        .map(|n| n.id)
        .collect::<std::collections::HashSet<_>>();
    result.notes.sort_by_key(|n| {
        (
            n.start,
            n.voice.index(),
            pinned.contains(&n.id),
            n.pitch,
            n.id,
        )
    });
    for voice in [Voice::Arp, Voice::Melody, Voice::Bass, Voice::Counterpoint] {
        let indices = result
            .notes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.voice == voice)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        for pair in indices.windows(2) {
            let start = result.notes[pair[1]].start;
            let previous = &mut result.notes[pair[0]];
            previous.length = previous.length.min(start - previous.start);
        }
    }
    result.notes.retain(|n| n.length > 0);
    result
        .notes
        .sort_by_key(|n| (n.start, n.pitch, n.voice.index(), n.id));
    // MIDI has one note state per pitch/channel: shorten repeated onsets,
    // collapse unisons, and keep the output the engine will actually play.
    let mut notes: Vec<Event> = Vec::new();
    for n in result.notes {
        if let Some(prev) = notes.iter_mut().rev().find(|p| p.pitch == n.pitch) {
            if prev.start == n.start {
                prev.length = prev.length.max(n.length);
                prev.velocity = prev.velocity.max(n.velocity);
                if pinned.contains(&n.id) {
                    prev.id = n.id;
                    prev.voice = n.voice;
                }
                continue;
            }
            prev.length = prev.length.min(n.start - prev.start);
        }
        notes.push(n);
    }
    result.notes = notes;
    result.warnings.sort();
    result.warnings.dedup();
    Ok(result)
}
fn nearest(pool: &[u8], target: i16) -> u8 {
    pool.iter()
        .copied()
        .min_by_key(|p| (i16::from(*p) - target).abs())
        .unwrap_or(60)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counterpoint_follows_the_edited_melody_and_ends_before_its_next_note() {
        let mut r = Recipe::default();
        r.voices[0].enabled = false;
        r.voices[2].enabled = true;
        r.voices[4].enabled = true;
        r.voices[4].rhythm = Rhythm::Quarter;
        r.voices[4].gate = 100;
        r.pin(Event {
            id: 999,
            voice: Voice::Melody,
            pitch: 71,
            start: 0,
            length: 24,
            velocity: 90,
        });
        let g = generate(&r).unwrap();
        assert!(g.notes.iter().any(|n| n.voice == Voice::Counterpoint));
        for counter in g.notes.iter().filter(|n| n.voice == Voice::Counterpoint) {
            for melody in g.notes.iter().filter(|n| {
                n.voice == Voice::Melody
                    && n.start < counter.start + counter.length
                    && n.start + n.length > counter.start
            }) {
                assert!(theory::is_consonant(melody.pitch, counter.pitch));
            }
        }
    }
    #[test]
    fn rules_are_checked_for_all_generated_voices_and_seeds() {
        let mut r = Recipe::default();
        r.progression("Cmaj7:16").unwrap();
        for v in &mut r.voices {
            v.enabled = true;
        }
        for seed in 0..64 {
            for v in &mut r.voices {
                v.seed = seed;
                v.motion = (seed % 4) as u8;
            }
            let g = generate(&r).unwrap();
            assert!(!g.notes.is_empty());
            assert!(g.notes.iter().all(|n| n.pitch % 12 != 5));
            assert_eq!(g, generate(&r).unwrap());
        }
    }
    #[test]
    fn triplets_swing_custom_gates_and_harmony_are_independent() {
        let mut r = Recipe::default();
        r.voices[0].rhythm = Rhythm::Triplet;
        let g = generate(&r).unwrap();
        assert!(g.notes.iter().any(|n| n.start == 16));
        r.voices[0].rhythm = Rhythm::Custom;
        r.voices[0].custom = vec![Gate {
            id: 1,
            start: 180,
            length: 80,
            velocity: 90,
        }];
        let g = generate(&r).unwrap();
        assert!(g.notes.iter().all(|n| n.start == 180 && n.length == 12));
        r.voices[0].rhythm = Rhythm::Eighth;
        r.voices[0].swing = 66;
        assert_eq!(gates(&r, Voice::Chords)[1].start, 31);
    }
    #[test]
    fn pinning_survives_regeneration_and_conflicts_refuse_send() {
        let mut r = Recipe::default();
        r.voices[2].enabled = true;
        let n = generate(&r)
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.voice == Voice::Melody)
            .unwrap();
        r.pin(n.clone());
        r.voices[2].seed = 88;
        assert!(generate(&r).unwrap().notes.contains(&n));
        let mut invalid = n;
        invalid.pitch = 65;
        r.pin(invalid);
        assert!(generate(&r).is_err());
    }
    #[test]
    fn pattern_retains_identity_exact_microtiming_and_recipe() {
        let mut r = Recipe::default();
        r.voices[0].rhythm = Rhythm::Triplet;
        let g = generate(&r).unwrap();
        let mut p = Pattern::empty(PatternId(42), "Keep".into());
        p.tag = "a7".into();
        write_pattern(&mut p, &r, &g.notes);
        assert_eq!(p.id, PatternId(42));
        assert_eq!(p.tag, "a7");
        assert!(p.trig(1).notes.iter().any(|n| n.micro_ticks == 4));
        assert_eq!(p.midi_lab, Some(r.clone()));
        assert_eq!(
            ron::from_str::<Recipe>(&ron::ser::to_string(&r).unwrap()).unwrap(),
            r
        );
    }
}
