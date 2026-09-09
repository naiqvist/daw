//! Author review material with the same renderer, instrument and export path as
//! MIDI Lab. Run: cargo run --example midi_composer_fixtures -- /tmp/midi-fixtures
use daw::midi_lab::{Destination, Recipe, Voice, audition::Job, composer::*};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or("/tmp/midi-fixtures".into()),
    );
    std::fs::create_dir_all(&directory)?;
    let mut fixtures = Vec::new();
    let mut base = Composition::default();
    base.voices[0].high = 71;
    base.name = "Chord leading · Cmaj7 Am7 Dm7 G7".into();
    fixtures.push(("chord-leading", base.clone()));
    for (name, movement, development) in [
        ("melody-answer", Movement::Steps, Development::Answer),
        ("melody-sequence", Movement::Sequence, Development::Sequence),
        ("melody-arpeggio", Movement::Arpeggio, Development::Invert),
    ] {
        let mut c = base.clone();
        c.name = name.into();
        let v = &mut c.voices[Voice::Melody.index()];
        v.enabled = true;
        v.low = 72;
        v.melody.movement = movement;
        v.melody.development = development;
        fixtures.push((name, c));
    }
    for (name, role) in [
        ("bass-foundation", BassRole::Foundation),
        ("bass-walking", BassRole::Walking),
        ("bass-riff", BassRole::Riff),
        ("bass-pedal", BassRole::Pedal),
        ("bass-sub", BassRole::Sub),
    ] {
        let mut c = base.clone();
        c.name = name.into();
        let v = &mut c.voices[Voice::Bass.index()];
        v.enabled = true;
        v.high = 47;
        v.bass.role = role;
        v.bass.fill_every = 4;
        if role == BassRole::Walking {
            v.bass.approaches = Decoration::Chromatic;
        }
        fixtures.push((name, c));
    }
    for (name, mut c) in fixtures {
        let r = render(&c)?;
        alternatives::save_snapshot(&mut c, name.into())?;
        std::fs::write(
            directory.join(format!("{name}.ron")),
            ron::ser::to_string_pretty(&c, ron::ser::PrettyConfig::default())?,
        )?;
        std::fs::write(
            directory.join(format!("{name}.mid")),
            output::midi(&r, 120)?,
        )?;
        let mut recipe = Recipe::composed();
        recipe.composition = Some(Box::new(c));
        let song = daw::sequencing::Song::default();
        let destination = Destination {
            track: song.tracks[0].id,
            pattern: song.patterns[0].id,
        };
        let job = Job::start(song, recipe, destination, vec![], 24_000);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let audio = loop {
            if let Some(audio) = job.take() {
                break audio?;
            }
            if std::time::Instant::now() > deadline {
                job.cancel();
                return Err(format!("Preview timed out: {name}").into());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        if audio.samples.iter().any(|s| !s.is_finite()) {
            return Err(format!("Nonfinite audio: {name}").into());
        }
        let mut wav = hound::WavWriter::create(
            directory.join(format!("{name}.wav")),
            hound::WavSpec {
                channels: 2,
                sample_rate: audio.rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for i in 0..audio.frames {
            wav.write_sample(audio.samples[i])?;
            wav.write_sample(audio.samples[audio.frames + i])?;
        }
        wav.finalize()?;
        println!("{name}: {} notes, {} beats", r.notes.len(), r.length / PPQ);
    }
    Ok(())
}
