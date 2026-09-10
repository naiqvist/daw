//! Read-only audit: preserve the previous take while adding expression locks.
use daw::params::acid as p;
use daw::sequencing::{PATTERN_STEP_TICKS, Song};
use serde::Deserialize;

#[derive(Deserialize)]
struct Document {
    song: Song,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: audit_expression_take baseline.stage.ron new.stage.ron".into());
    }
    let a: Document = ron::from_str(&std::fs::read_to_string(&args[0])?)?;
    let b: Document = ron::from_str(&std::fs::read_to_string(&args[1])?)?;
    assert_eq!(a.song.tracks.len(), 3);
    assert_eq!(b.song.tracks.len(), 3);
    assert_eq!(a.song.patterns.len(), b.song.patterns.len());
    assert_eq!(a.song.bpm, b.song.bpm);
    assert_eq!(a.song.tempo, b.song.tempo);
    let time = daw::tempo::TempoTable::build(&b.song, 48_000.0, b.song.bpm as f64);
    let mut ornaments = [0usize; 7];
    let mut count = 0;
    for (index, (old, new)) in a.song.patterns.iter().zip(&b.song.patterns).enumerate() {
        assert_eq!(old.length_ticks, new.length_ticks);
        assert_eq!(old.step_count(), new.step_count());
        for step in 0..old.step_count() {
            let o = old.trig(step);
            let n = new.trig(step);
            assert_eq!(
                o.notes,
                n.notes,
                "notes changed in track {} tick {}",
                index + 1,
                step * PATTERN_STEP_TICKS
            );
            assert_eq!(o.enabled, n.enabled);
            assert_eq!(o.probability, n.probability);
            assert_eq!(o.cond, n.cond);
            assert_eq!(o.retrig, n.retrig);
            assert_eq!(o.sound, n.sound);
            for lock in &o.locks {
                assert!(n.locks.contains(lock), "old lock removed");
            }
            count += n.notes.len();
            if let Some(mode) = n.lock(p::ORNAMENT) {
                assert!(!n.notes.is_empty(), "ornament without note-on");
                let mode = mode.round() as usize;
                assert!((1..=6).contains(&mode));
                ornaments[mode] += 1;
                println!(
                    "track {} tick {} ({:.3} s): {}",
                    index + 1,
                    step * PATTERN_STEP_TICKS,
                    time.sample_at(step * PATTERN_STEP_TICKS) as f64 / 48_000.0,
                    p::ORNAMENT_NAMES[mode]
                );
            }
        }
    }
    assert_eq!(count, 92);
    assert!(
        ornaments[1..].iter().all(|n| *n > 0),
        "not all six ornaments used"
    );
    for (old, new) in a.song.tracks.iter().zip(&b.song.tracks) {
        assert_eq!(old.blocks, new.blocks);
        assert_eq!(old.pan, new.pan);
        assert_eq!(old.volume, new.volume);
        assert_eq!(old.strip, new.strip, "space/delay settings changed");
        let old = old.machine.as_ref().unwrap();
        let new = new.machine.as_ref().unwrap();
        assert_eq!(old.kind, new.kind);
        for id in 0..=p::LEVEL {
            assert_eq!(old.value(id), new.value(id));
        }
        assert_eq!(new.value(p::VIBRATO_INTENSITY), 0.0);
        assert_eq!(new.value(p::ORNAMENT), 0.0);
    }
    println!(
        "PASS: all 92 notes/gates/velocities and original sound/space preserved; all six ornaments on note onsets; expression defaults off."
    );
    Ok(())
}
