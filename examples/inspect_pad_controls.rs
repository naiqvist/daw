use daw::{
    devices::DeviceKind,
    pages::{PageKey, resolve},
    sequencing::Song,
};
fn main() {
    if let Some(path) = std::env::args().nth(1) {
        #[derive(serde::Deserialize)]
        struct Document { song: Song }
        let document: Document = ron::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        println!("BPM at start {}", document.song.bpm_at(0, 120.0));
        for (index, track) in document.song.tracks.iter().enumerate() {
            println!("track {} volume {} blocks {:?}", index+1, track.volume, track.blocks);
            if let Some(machine) = &track.machine {
                println!("instrument {:?}", machine.kind);
                for def in machine.kind.spec().params {
                    println!("{} = {}", def.name, machine.value(def.id));
                }
            }
            if let Some(room) = document.song.section(index, daw::console::SectionKind::Room) {
                println!("room bypassed {}", room.bypassed);
                for def in room.kind.spec().params { println!("room {} = {}", def.name, room.value(def.id)); }
            }
        }
        for pattern in &document.song.patterns {
            for step in 0..pattern.step_count() {
                for note in &pattern.trig(step).notes {
                    println!("note midi {} tick {} gate {} velocity {}", daw::pitch::nearest_midi(note.pitch.resolve(&document.song.key)), step*12 + note.micro_ticks as usize, note.length_ticks, note.velocity);
                }
            }
        }
        return;
    }
    let mut song = Song::default();
    song.add_device(0, DeviceKind::Table).unwrap();
    for key in [PageKey::Src, PageKey::Fltr, PageKey::Amp, PageKey::Fx] {
        for (i, page) in resolve(&song.tracks[0], key).iter().enumerate() {
            println!("{key:?} page {} {}: {:?}", i + 1, page.title, page.slots);
        }
    }
    println!("LFO: {:?}", song.tracks[0].lfos);
    for def in DeviceKind::Console(daw::console::SectionKind::Room)
        .spec()
        .params
    {
        println!(
            "room {} {} range {}..{} default {}",
            def.id, def.name, def.min, def.max, def.default
        );
    }
}
