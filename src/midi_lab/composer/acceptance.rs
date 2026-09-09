use super::*;
use crate::{
    midi_lab::{Recipe, Voice},
    theory::{harmony::HarmonicStyle, material::Material},
};
fn sound(r: &Rendered) -> Vec<(usize, u8, u32, u32, u8)> {
    {
        let mut v = r
            .notes
            .iter()
            .map(|n| (n.voice.index(), n.pitch, n.start, n.length, n.velocity))
            .collect::<Vec<_>>();
        v.sort();
        v
    }
}
#[test]
fn all_nonempty_twelve_tet_collections_survive_the_complete_generator() {
    let mut c = Composition::default();
    c.key = None;
    c.harmony.truncate(1);
    c.length = 192;
    c.looping = false;
    c.harmony[0].voicing.lead = false;
    for mask in 1u16..4096 {
        c.harmony[0].material = Material::parse(&format!(
            "pc:{}",
            (0..12)
                .filter(|pc| mask & (1 << pc) != 0)
                .map(crate::theory::pitch_class_name)
                .collect::<Vec<_>>()
                .join(",")
        ))
        .unwrap();
        let a = render(&c).unwrap();
        let actual = a.notes.iter().fold(0u16, |m, n| m | (1 << (n.pitch % 12)));
        assert_eq!(actual, mask);
        assert_eq!(a.notes.len(), mask.count_ones() as usize);
        assert!(
            a.notes
                .iter()
                .all(|n| n.provenance.harmony == Some(c.harmony[0].id))
        );
    }
}
#[test]
fn exact_octaves_and_unison_identities_survive_midi_channel_allocation() {
    let mut c = Composition::default();
    c.harmony.truncate(1);
    c.harmony[0].material = Material::parse("notes:Cb4,B3,E#4,G4").unwrap();
    let r = render(&c).unwrap();
    assert_eq!(
        r.notes.iter().map(|n| n.pitch).collect::<Vec<_>>(),
        [59, 59, 65, 67]
    );
    assert_eq!(output::lanes(&r.notes).len(), 2);
    assert_eq!(
        output::lanes(&r.notes).iter().map(Vec::len).sum::<usize>(),
        4
    );
    let midi = output::midi(&r, 120).unwrap();
    assert_eq!(&midi[10..12], &3u16.to_be_bytes());
}
#[test]
fn adopting_a_legacy_snapshot_keeps_every_sounding_value() {
    let mut old = Recipe::default();
    old.voices[2].enabled = true;
    old.voices[3].enabled = true;
    let mut c = bridge::migrate(&old).unwrap();
    let before = render(&c).unwrap();
    bridge::thaw(&mut c).unwrap();
    let after = render(&c).unwrap();
    assert_eq!(sound(&before), sound(&after));
}
#[test]
fn newer_engine_plays_its_saved_snapshot_without_regenerating() {
    let mut c = Composition::default();
    let expected = render(&c).unwrap();
    alternatives::save_snapshot(&mut c, "saved".into()).unwrap();
    c.engine = 999;
    c.harmony[0].material = Material::parse("F#").unwrap();
    assert_eq!(sound(&expected), sound(&render(&c).unwrap()));
}

#[test]
fn frozen_cache_identity_tracks_saved_notes_without_recursive_save_growth() {
    let mut c = bridge::migrate(&Recipe::default()).unwrap();
    let original = c.input().unwrap();
    c.snapshot.as_mut().unwrap().events[0].velocity = 37;
    assert_ne!(c.input().unwrap(), original);
    alternatives::save_snapshot(&mut c, "first".into()).unwrap();
    let first = c.input().unwrap();
    alternatives::save_snapshot(&mut c, "second".into()).unwrap();
    assert_eq!(c.input().unwrap(), first);
    let loaded: Composition = ron::from_str(&ron::to_string(&c).unwrap()).unwrap();
    assert_eq!(
        sound(&render(&loaded).unwrap()),
        sound(&render(&c).unwrap())
    );
}
#[test]
fn strict_policy_is_selected_and_arbitrary_notes_remain_authorable() {
    let mut c = Composition::default();
    c.harmony.truncate(1);
    c.harmony[0].material = Material::parse("Cmaj7add11").unwrap();
    assert!(render(&c).is_ok());
    c.voices[0].profile = Some(HarmonicStyle::Strict);
    assert!(render(&c).is_err());
    c.harmony[0].material = Material::parse("notes:C4,E4,F4,B4").unwrap();
    assert!(render(&c).is_ok());
}
#[test]
fn bass_roles_have_distinct_musical_outputs_and_obey_their_range() {
    let mut c = Composition::default();
    c.voices[0].enabled = false;
    c.voices[3].enabled = true;
    let mut signatures = Vec::new();
    for role in BassRole::ALL {
        c.voices[3].bass.role = *role;
        let r = render(&c).unwrap();
        assert!(r.notes.iter().all(|n| (28..=60).contains(&n.pitch)));
        signatures.push(sound(&r));
    }
    signatures.sort();
    signatures.dedup();
    assert_eq!(signatures.len(), 6);
}
#[test]
fn slash_bass_and_altered_collections_use_the_actual_members() {
    let mut c = Composition::default();
    c.progression("C7b5/Eb:4").unwrap();
    c.voices[0].enabled = false;
    c.voices[3].enabled = true;
    c.voices[3].bass.fill_every = 0;
    let r = render(&c).unwrap();
    assert!(r.notes.iter().all(|n| n.pitch % 12 == 3));
    c.harmony[0].material = Material::parse("C7b5#5b9#9").unwrap();
    c.voices[3].bass.role = BassRole::Riff;
    let r = render(&c).unwrap();
    let mask = c.harmony[0].material.mask();
    assert!(r.notes.iter().all(|n| mask & (1 << (n.pitch % 12)) != 0));
}
#[test]
fn form_snapshot_saves_the_expanded_duration_and_local_edits() {
    let mut c = Composition::default();
    c.sections.push(Section {
        id: 10,
        name: "A".into(),
        start: 0,
        length: 768,
        source: None,
        transpose: 0,
        diatonic: false,
        enabled: [true; 5],
        simplify_bass: false,
    });
    c.form = vec![10, 10];
    let expected = render(&c).unwrap();
    alternatives::save_snapshot(&mut c, "AA".into()).unwrap();
    assert_eq!(c.snapshot.as_ref().unwrap().length, 1536);
    c.frozen = true;
    let reopened: Composition = ron::from_str(&ron::to_string(&c).unwrap()).unwrap();
    assert_eq!(sound(&render(&reopened).unwrap()), sound(&expected));
    assert_eq!(render(&reopened).unwrap().length, 1536);
}
#[test]
fn velocity_shape_changes_expression_without_moving_locked_events() {
    let mut c = Composition::default();
    c.voices[0].enabled = false;
    c.voices[2].enabled = true;
    let original = render(&c).unwrap();
    c.pin(&original.notes[0]);
    c.voices[2].velocity_curve = vec![(0, 100), (1000, 10)];
    let next = render(&c).unwrap();
    assert_eq!(original.notes[0].velocity, next.notes[0].velocity);
    assert!(
        original
            .notes
            .iter()
            .zip(&next.notes)
            .any(|(a, b)| a.velocity != b.velocity)
    );
    assert!(
        original
            .notes
            .iter()
            .zip(&next.notes)
            .all(|(a, b)| (a.pitch, a.start, a.length) == (b.pitch, b.start, b.length))
    );
}
#[test]
fn malformed_resource_requests_refuse_without_panicking() {
    let mut c = Composition::default();
    c.voices[2].rhythm.steps = 0;
    assert!(render(&c).is_err());
    assert!(rhythm::pulses(&c, Voice::Melody).is_err());
    let mut c = Composition::default();
    c.voices[0].rhythm.kind = RhythmKind::Custom;
    c.voices[0].rhythm.custom_length = 0;
    assert!(render(&c).is_err());
    let mut c = Composition::default();
    c.harmony[0].material = Material::parse("notes:C4,E4").unwrap();
    c.voices[0].high = 50;
    assert!(render(&c).is_err());
}
#[test]
fn literal_canon_keeps_its_entire_delayed_tail() {
    let mut c = Composition::default();
    c.voices[0].enabled = false;
    c.voices[2].enabled = true;
    c.voices[4].enabled = true;
    c.voices[4].low = 24;
    c.voices[4].high = 96;
    c.voices[4].counter.species = Species::Canon;
    let r = render(&c).unwrap();
    let upper = r
        .notes
        .iter()
        .filter(|n| n.voice == Voice::Melody)
        .collect::<Vec<_>>();
    let lower = r
        .notes
        .iter()
        .filter(|n| n.voice == Voice::Counterpoint)
        .collect::<Vec<_>>();
    assert_eq!(upper.len(), lower.len());
    assert!(r.length > c.length);
    for a in upper {
        assert!(lower.iter().any(|b| b.start == a.start + 48
            && b.length == a.length
            && i16::from(b.pitch) == i16::from(a.pitch) - 12));
    }
}

#[test]
fn repeated_melody_uses_the_opening_motif() {
    let mut c = Composition::default();
    c.progression("Cmaj7:4 Cmaj7:4").unwrap();
    c.voices[0].enabled = false;
    c.voices[2].enabled = true;
    c.voices[2].melody.development = Development::Repeat;
    let r = render(&c).unwrap();
    let first = r
        .notes
        .iter()
        .filter(|n| n.start < 192)
        .map(|n| (n.start, n.pitch))
        .collect::<Vec<_>>();
    let next = r
        .notes
        .iter()
        .filter(|n| n.start >= 192)
        .map(|n| (n.start - 192, n.pitch))
        .collect::<Vec<_>>();
    assert_eq!(first, next);
}
#[test]
fn motif_alternatives_transform_actual_written_material() {
    let mut c = Composition::default();
    c.voices[0].enabled = false;
    c.voices[2].enabled = true;
    c.progression("Cmaj7:4").unwrap();
    let r = render(&c).unwrap();
    let id = motif::capture(&mut c, &r.notes, Voice::Melody, 0, 192, "Opening".into()).unwrap();
    let placement = c.mint();
    c.placements.push(Placement {
        id: placement,
        motif: id,
        voice: Voice::Melody,
        start: 0,
        anchor: 60,
        transforms: vec![],
    });
    let candidates = alternatives::explore(&c, Voice::Melody, VariationAction::Develop, 4).unwrap();
    assert!(candidates.len() >= 2);
    assert!(
        candidates
            .iter()
            .all(|a| a.recipe.placements != c.placements)
    );
}
#[test]
fn species_profiles_produce_checked_declared_patterns() {
    let mut c = Composition::default();
    c.progression("C:4 Dm:4 Em:4 Dm:4 C:4").unwrap();
    c.voices[0].enabled = false;
    c.voices[2].enabled = true;
    c.voices[2].low = 60;
    c.voices[2].high = 76;
    c.voices[2].rhythm.kind = RhythmKind::Hold;
    c.voices[2].rhythm.gate = 100;
    c.voices[2].melody.targets = TargetKind::Root;
    c.voices[4].enabled = true;
    c.voices[4].low = 36;
    c.voices[4].high = 59;
    for species in [
        Species::First,
        Species::Second,
        Species::Third,
        Species::Fourth,
        Species::Fifth,
    ] {
        c.voices[4].counter.species = species;
        let r = render(&c).unwrap_or_else(|e| panic!("{species:?}: {e}"));
        assert!(r.notes.iter().any(|n| n.voice == Voice::Counterpoint));
        assert!(
            !r.findings
                .iter()
                .any(|f| f.rule.starts_with("counterpoint."))
        );
    }
}
#[test]
fn a_form_instance_note_can_be_inserted_after_the_source_timeline() {
    let mut c = Composition::default();
    c.voices[Voice::Melody.index()].enabled = true;
    c.sections.push(Section {
        id: 10,
        name: "A".into(),
        start: 0,
        length: 768,
        source: None,
        transpose: 0,
        diatonic: false,
        enabled: [true; 5],
        simplify_bass: false,
    });
    c.form = vec![10, 10];
    let n = NoteEvent {
        id: 9000,
        voice: Voice::Melody,
        member: None,
        pitch: 72,
        start: 1000,
        length: 24,
        velocity: 80,
        provenance: Provenance::new("manual.note", None, "Written in the repeated section"),
    };
    c.overrides.push(Override {
        id: n.id,
        instance: true,
        inserted: Some(n.clone()),
        ..Override::default()
    });
    let r = render(&c).unwrap();
    assert!(
        r.notes.contains(&n)
            || r.notes
                .iter()
                .any(|m| m.id == n.id && m.start == 1000 && m.pitch == 72)
    );
    assert_eq!(r.length, 1536);
}

#[test]
fn source_locks_and_destinations_flow_through_form_with_local_precedence() {
    let mut c = Composition::default();
    c.voices[0].high = 108;
    let note = render(&c).unwrap().notes[0].clone();
    c.pin(&note);
    c.event_destinations.insert(
        note.id,
        crate::midi_lab::Destination {
            track: crate::sequencing::TrackId(9),
            pattern: crate::sequencing::PatternId(9),
        },
    );
    c.sections.push(Section {
        id: 10,
        name: "A".into(),
        start: 0,
        length: c.length,
        source: None,
        transpose: 0,
        diatonic: false,
        enabled: [true; 5],
        simplify_bass: false,
    });
    c.form = vec![10, 10];
    let first = render(&c)
        .unwrap()
        .notes
        .iter()
        .find(|n| n.provenance.origin == Some(note.id))
        .unwrap()
        .clone();
    c.overrides.push(Override {
        id: first.id,
        instance: true,
        pitch: Some(note.pitch + 1),
        ..Override::default()
    });
    c.tension = vec![(0, 100), (1000, 100)];
    c.tension_map.register = 1;
    c.tension_map.velocity = 20;
    c.tension_map.gate = 20;
    let r = render(&c).unwrap();
    let copies = r
        .notes
        .iter()
        .filter(|n| n.provenance.origin == Some(note.id))
        .collect::<Vec<_>>();
    assert_eq!(copies.len(), 2);
    assert_eq!(copies[0].pitch, note.pitch + 1);
    assert_eq!(copies[1].pitch, note.pitch);
    for n in copies {
        assert_eq!((n.length, n.velocity), (note.length, note.velocity));
    }
    let deliveries = output::deliveries(
        &c,
        &r,
        crate::midi_lab::Destination {
            track: crate::sequencing::TrackId(1),
            pattern: crate::sequencing::PatternId(1),
        },
    )
    .unwrap();
    assert_eq!(
        deliveries
            .iter()
            .find(|d| d.destination.track.0 == 9)
            .unwrap()
            .events
            .len(),
        2
    );
}

#[test]
fn held_chords_and_sub_bass_use_the_declared_gate_without_moving_attacks() {
    for voice in [Voice::Chords, Voice::Bass] {
        let mut c = Composition::default();
        c.voices.iter_mut().for_each(|v| v.enabled = false);
        c.voices[voice.index()].enabled = true;
        c.voices[voice.index()].bass.role = BassRole::Sub;
        c.voices[voice.index()].rhythm.gate = 100;
        let full = render(&c).unwrap();
        c.voices[voice.index()].rhythm.gate = 50;
        let half = render(&c).unwrap();
        assert_eq!(full.notes.len(), half.notes.len());
        for (a, b) in full.notes.iter().zip(&half.notes) {
            assert_eq!((a.start, a.pitch, a.id), (b.start, b.pitch, b.id));
            assert_eq!(a.length, b.length * 2);
        }
    }
}
