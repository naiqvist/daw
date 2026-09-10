//! The spectral instrument uses the ordinary native palette and parameter deck.
use super::{Stage, StageIntent};
use crate::{devices::DeviceKind, pages::PageKey};
pub(super) const COMMANDS: &[crate::ui::palette::TypedCommand] = &[
    crate::ui::palette::TypedCommand {
        name: "spectral",
        usage: "spectral focus <harmonic> | bins <all|odd|even|1:16> <amp|phase> <a[:b]> | load/save <path> | add/connect/fx/source/mod/route/depth/inspect",
    },
];
impl Stage {
    pub(super) fn apply_spectral_command(&mut self, input: &str) -> bool {
        let words: Vec<_> = input.split_whitespace().skip(1).collect();
        let result = (|| -> Result<String, String> {
            let track = self
                .deck_track()
                .or_else(|| self.addressed_track())
                .ok_or("no track under the cursor")?;
            let machine = self.song.tracks[track]
                .machine
                .as_ref()
                .ok_or("add a spectral instrument first")?;
            if machine.kind != DeviceKind::Spectral {
                return Err("select a spectral instrument first".into());
            }
            if let ["focus", harmonic] = words.as_slice() {
                let h = harmonic
                    .parse::<usize>()
                    .map_err(|_| "harmonic must be 1..128")?;
                if !(1..=128).contains(&h) {
                    return Err("harmonic must be 1..128".into());
                }
                let _ = self.apply(StageIntent::Page(PageKey::Src));
                let id = self.song.tracks[track].id;
                self.deck.sub.entry(id).or_insert([0; 8])[PageKey::Src.index()] = (h - 1) / 4;
                self.deck.slot = ((h - 1) % 4) * 2;
                return Ok(format!("harmonic {h:03} · amplitude / phase"));
            }
            let mut candidate = machine.clone();
            let result = crate::spectral::command(&mut candidate, &words)?;
            if &candidate != machine {
                self.song.tracks[track].machine = Some(candidate);
                self.touched();
                self.remixed();
                self.settle();
            }
            Ok(result)
        })();
        match result {
            Ok(message) => {
                self.notice = Some(format!("spectral · {message}"));
                true
            }
            Err(error) => {
                self.notice = Some(format!("REFUSED spectral · {error}"));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::spectral as p;
    fn stage() -> Stage {
        let mut stage = Stage::new();
        stage.song.add_device(0, DeviceKind::Spectral).unwrap();
        stage.settle();
        stage
    }
    #[test]
    fn spectral_native_pages_and_parameter_letters_cover_every_harmonic() {
        let mut s = stage();
        let table = crate::pages::key_table(DeviceKind::Spectral).unwrap();
        let mut ids = std::collections::BTreeSet::new();
        for key in table.iter().flatten() {
            for page in key.subpages {
                for id in page.slots.iter().flatten() {
                    ids.insert(*id);
                }
            }
        }
        assert_eq!(ids.len(), p::COUNT);
        assert!(s.apply_timeline_command("param spectral: attack=140ms; release=1700ms; h128_amp=0.45; h128_phase=120deg; ornament=murki"));
        let machine = s.song.tracks[0].machine.as_ref().unwrap();
        assert_eq!(machine.value(p::amp(127)), 0.45);
        assert_eq!(machine.value(p::phase(127)), 120.0);
        assert_eq!(machine.value(p::ORNAMENT), 6.0);
        assert!(s.apply_timeline_command("spectral focus 128"));
        assert_eq!(s.deck.slot, 6);
        assert_eq!(s.deck.sub[&s.song.tracks[0].id][PageKey::Src.index()], 31);
        for row in p::TABLE {
            for n in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let value = p::value(row.id, n);
                let back = p::value(row.id, p::norm(row.id, value));
                assert!((value - back).abs() < 0.02, "{}", row.name);
            }
        }
    }
    #[test]
    fn spectral_native_routing_refuses_atomically_and_undoes_once() {
        let mut s = stage();
        for command in [
            "spectral add tone filter",
            "spectral connect input tone 1",
            "spectral connect tone output 0.7",
            "spectral disconnect input output",
            "spectral source breath lfo shared",
            "spectral route breath fx.tone.hz 1200",
            "spectral mod breath hz 0.3",
            "spectral source rise envelope voice",
            "spectral route rise pitch 0.25",
            "spectral source brightness macro shared",
            "spectral mod brightness index 8",
            "spectral route brightness fx.tone.hz 2400",
            "param spectral: macro 8=0.4; mono=legato; glide=70ms; pitch ornament=andolan; vibrato depth=12ct",
        ] {
            assert!(
                s.apply_timeline_command(command),
                "{command}: {:?}",
                s.notice
            );
        }
        let before = s.song.clone();
        let rev = s.revision();
        for bad in [
            "spectral connect tone tone 1",
            "spectral fx tone hz NaN",
            "spectral route rise shift 4",
            "spectral bins 0:3 amp 1",
            "spectral source breath random shared",
            "spectral route missing amp 1",
        ] {
            assert!(!s.apply_timeline_command(bad), "{bad}");
            assert_eq!(s.song, before);
            assert_eq!(s.revision(), rev);
        }
        assert!(s.apply_timeline_command("spectral bins odd amp 0.7:0.1"));
        assert_ne!(s.song, before);
        assert!(s.history.undo(&mut s.song));
        assert_eq!(s.song, before);
    }
    #[test]
    fn spectral_song_and_sound_recall_preserve_routing_and_overrides() {
        let mut s = stage();
        for command in [
            "spectral add echo delay",
            "spectral connect input echo 1",
            "spectral connect echo output 0.2",
            "spectral source drift random voice",
            "spectral route drift pitch 0.07",
            "param spectral.h017_phase = 150deg",
        ] {
            assert!(s.apply_timeline_command(command), "{command}");
        }
        let text = ron::to_string(&s.song).unwrap();
        let recalled: crate::sequencing::Song = ron::from_str(&text).unwrap();
        assert_eq!(recalled, s.song);
        let sound = crate::sound::Sound::capture(&s.song.tracks[0]);
        let text = ron::to_string(&sound).unwrap();
        let sound: crate::sound::Sound = ron::from_str(&text).unwrap();
        let machine = sound.machine_device().unwrap();
        assert_eq!(
            machine.spectral,
            s.song.tracks[0].machine.as_ref().unwrap().spectral
        );
        assert_eq!(machine.value(p::phase(16)), 150.0);
        let mut fresh = crate::sequencing::Song::default();
        assert!(fresh.load_sound(0, &sound).is_empty());
        assert_eq!(
            fresh.tracks[0].machine.as_ref().unwrap().spectral,
            machine.spectral
        );
    }
}
