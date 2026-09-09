//! Bake ROM's factory bank ahead of time.
//!
//!     cargo run --example rom_bake            # the device rate, 48 kHz
//!     cargo run --example rom_bake -- 44100
//!
//! Every recipe in `audio::rom::bank` is rendered to `~/Corpus/daw/rom`
//! if it is not already there. Nothing is ever overwritten: a changed
//! recipe writes a new file under a new fingerprint, so an old bank stays
//! playable until it is swept by hand.
//!
//! The app does this on its own the first time a ROM track is built —
//! this is the way to pay the cost before a session rather than during
//! one, and the way to see what the bank actually costs on disk.

use daw::audio::rom::bank;

fn main() {
    let rate: u32 = std::env::args()
        .nth(1)
        .and_then(|word| word.parse().ok())
        .unwrap_or(48_000);
    println!("baking {} recipes at {rate} Hz", bank::RECIPES.len());
    println!("into {}", bank::cache_dir().display());

    let mut written = 0usize;
    let mut kept = 0usize;
    let mut bytes = 0u64;
    let mut failed = 0usize;
    for recipe in bank::RECIPES {
        let existed = bank::wav_path(recipe, rate).exists();
        match bank::bake(recipe, rate) {
            Ok(path) => {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                bytes += size;
                if existed {
                    kept += 1;
                } else {
                    written += 1;
                    let layout = recipe.layout(rate);
                    println!(
                        "  {:<14} {:>7} frames  {:>7.1} Hz  {}",
                        recipe.name,
                        layout.frames,
                        layout.hz * layout.correction,
                        if layout.looped { "looped" } else { "one-shot" }
                    );
                }
            }
            Err(error) => {
                failed += 1;
                eprintln!("  {}: {error}", recipe.name);
            }
        }
    }
    println!(
        "{written} written, {kept} already there, {failed} failed — {:.1} MB",
        bytes as f64 / 1_048_576.0
    );
    // The multisamples the PCM cell walks, so the bank's shape is visible
    // beside its cost.
    for multi in bank::MULTIS {
        println!(
            "  {:<8} {:<7} {} zones",
            multi.category,
            multi.name,
            multi.zones.len()
        );
    }
}
