//! Write ROM's factory programs into the sound library.
//!
//!     cargo run --example rom_presets              # into ~/Corpus/daw/sounds
//!     cargo run --example rom_presets -- /tmp/try  # somewhere else
//!
//! Eight jungle pads, each carrying its own effects — the tape colour,
//! the dotted-eighth echo and the hall are part of the program, not
//! something to dial in after loading it. They arrive in the browser's
//! Sounds shelf under the plain lane, and a step can lock a whole one.
//!
//! The bank they play is baked by `rom_bake` (or by the graph, the first
//! time a ROM track compiles).

use daw::audio::rom::presets::PADS;

fn main() {
    let into = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(daw::sound::dir);
    println!("writing {} programs into {}", PADS.len(), into.display());
    let mut written = 0usize;
    for pad in PADS {
        let sound = pad.sound();
        match daw::sound::save(&into, pad.name, &sound) {
            Ok(path) => {
                written += 1;
                let machine = sound.machine.as_ref().map_or(0, |m| m.overrides.len());
                let fx: Vec<&str> = sound
                    .sections
                    .iter()
                    .filter(|section| section.in_)
                    .map(|section| section.kind.name())
                    .collect();
                println!(
                    "  {:<14} {} + {:<6} {:>2} cells  fx: {:<20} {}",
                    pad.name,
                    pad.pcm[0],
                    pad.pcm[1],
                    machine,
                    fx.join(" "),
                    path.file_name().unwrap_or_default().to_string_lossy()
                );
                println!("  {:<14} {}", "", pad.note);
            }
            Err(error) => eprintln!("  {}: {error}", pad.name),
        }
    }
    println!("{written} written");
}
