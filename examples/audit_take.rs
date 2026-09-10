//! Compare musical state in two stage projects after a workflow retest.
use daw::sequencing::Song;
use serde::Deserialize;

#[derive(Deserialize)]
struct Document {
    song: Song,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    if paths.len() != 2 {
        return Err("usage: audit_take <baseline.stage.ron> <retest.stage.ron>".into());
    }
    let a: Document = ron::from_str(&std::fs::read_to_string(&paths[0])?)?;
    let b: Document = ron::from_str(&std::fs::read_to_string(&paths[1])?)?;
    assert_eq!(a.song.tracks.len(), b.song.tracks.len(), "track count");
    assert_eq!(
        a.song.patterns.len(),
        b.song.patterns.len(),
        "pattern count"
    );
    let mut note_count = 0;
    for (index, (old, new)) in a.song.patterns.iter().zip(&b.song.patterns).enumerate() {
        assert_eq!(
            old.length_ticks,
            new.length_ticks,
            "pattern {} duration",
            index + 1
        );
        assert_eq!(old.swing, new.swing, "pattern {} swing", index + 1);
        assert_eq!(old.scale, new.scale, "pattern {} scale", index + 1);
        assert_eq!(
            old.step_count(),
            new.step_count(),
            "pattern {} steps",
            index + 1
        );
        for step in 0..old.step_count() {
            assert_eq!(
                old.trig(step),
                new.trig(step),
                "pattern {} step {}: notes, timing, velocity, pitch, locks, or conditions differ",
                index + 1,
                step
            );
            note_count += old.trig(step).notes.len();
        }
        println!("pattern {}: exact match", index + 1);
    }
    for (index, (old, new)) in a.song.tracks.iter().zip(&b.song.tracks).enumerate() {
        assert_eq!(
            old,
            new,
            "track {}: arrangement, instruments, or mix differs",
            index + 1
        );
    }
    assert_eq!(a.song, b.song, "remaining song state differs");
    println!(
        "PASS: entire song matches; {note_count} notes, {} tracks",
        a.song.tracks.len()
    );
    Ok(())
}
