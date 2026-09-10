//! An authored score that emits and preflights ordinary native palette commands.
//! No new app command, pre-rendered backing track, or hidden note import.
//! cargo run --example lagoon_drive -- /absolute/fresh/output-directory
use daw::{
    audio::{
        bounce::{BounceFormat, BounceOptions, bounce},
        spectral::SpectralPatch,
        spectral_fx as fx, spectral_mod as mo,
    },
    devices::DeviceKind,
    sequencing::{Locator, Pattern, PatternId, Song, TrackKind},
    ui::stage::Stage,
};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const BAR: usize = 192;
const SECTION: usize = 16 * BAR;
const NAMES: [&str; 8] = [
    "Undertow",
    "Porcelain",
    "Salt Air",
    "Splinters",
    "Black Current",
    "Water Piano",
    "Moon Veil",
    "Firefly",
];
const SECTIONS: [&str; 8] = [
    "Moon on Water",
    "The Body Arrives",
    "Refractions",
    "Broken Current",
    "Suspended Room",
    "Return Through Glass",
    "Luminous Machinery",
    "The Tide Leaves",
];
const KINDS: [DeviceKind; 8] = [
    DeviceKind::Thump,
    DeviceKind::Clay,
    DeviceKind::Drum,
    DeviceKind::Sampler,
    DeviceKind::Spectral,
    DeviceKind::Glass,
    DeviceKind::Poly,
    DeviceKind::Acid,
];
const SAMPLE: &str = "/home/naiqvist/Samples/MusicRadar-90s-hip-hop/90s Hip Hop/Beats/Beat_01(95BPM)/Beat_01_Drums_Conga.wav";
const FULL_RENDER: &str = "/home/naiqvist/Music/daw/renders/lagoon-of-broken-glass-v1.wav";

fn write_new(path: &Path, data: &str) -> Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(data.as_bytes())?;
    Ok(())
}
fn csv<T: ToString>(items: impl IntoIterator<Item = T>) -> String {
    items
        .into_iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",")
}
fn patch() -> SpectralPatch {
    let mut p = SpectralPatch::default();
    let node = |id: &str, module| fx::Node {
        id: id.into(),
        module,
    };
    p.fx = fx::Patch {
        version: 1,
        nodes: vec![
            node(
                "sub",
                fx::Module::Filter {
                    mode: fx::FilterMode::Lowpass,
                    hz: 125.,
                    q: 0.707,
                },
            ),
            node(
                "upper",
                fx::Module::Filter {
                    mode: fx::FilterMode::Highpass,
                    hz: 180.,
                    q: 0.707,
                },
            ),
            node(
                "fold",
                fx::Module::Shape {
                    mode: fx::ShapeMode::Fold,
                    drive: 2.2,
                    bias: 0.01,
                    mix: 0.5,
                },
            ),
            node(
                "vowel",
                fx::Module::Filter {
                    mode: fx::FilterMode::Lowpass,
                    hz: 650.,
                    q: 0.85,
                },
            ),
            node(
                "smear",
                fx::Module::Disperser {
                    hz: 740.,
                    q: 0.7,
                    stages: 4,
                },
            ),
            node("colour", fx::Module::Gain { gain: 0.06 }),
            node("dc", fx::Module::DcBlock),
        ],
        routes: vec![
            fx::Route::new("input", "sub", 1.),
            fx::Route::new("sub", "dc", 1.),
            fx::Route::new("input", "upper", 1.),
            fx::Route::new("upper", "fold", 1.),
            fx::Route::new("fold", "vowel", 1.),
            fx::Route::new("vowel", "smear", 1.),
            fx::Route::new("smear", "colour", 1.),
            fx::Route::new("colour", "dc", 1.),
            fx::Route::new("dc", "output", 1.),
        ],
    };
    let route = |source: &str, target, depth| mo::Route {
        source: source.into(),
        target,
        depth,
        offset: 0.,
        polarity: mo::Polarity::Native,
    };
    let target = |node: &str, control| mo::Target::Fx {
        node: node.into(),
        control,
    };
    p.modulation.sources = vec![
        mo::Source {
            id: "bloom".into(),
            scope: mo::Scope::Shared,
            generator: mo::Generator::Macro { index: 1 },
        },
        mo::Source {
            id: "mouth".into(),
            scope: mo::Scope::Shared,
            generator: mo::Generator::Macro { index: 2 },
        },
        mo::Source {
            id: "grain".into(),
            scope: mo::Scope::Shared,
            generator: mo::Generator::Macro { index: 3 },
        },
        mo::Source {
            id: "breath".into(),
            scope: mo::Scope::Shared,
            generator: mo::Generator::Lfo {
                shape: mo::Shape::Sine,
                hz: 0.09,
                phase: 0.,
                retrigger: false,
            },
        },
        mo::Source {
            id: "pulse".into(),
            scope: mo::Scope::Shared,
            generator: mo::Generator::Lfo {
                shape: mo::Shape::Sine,
                hz: 3.6,
                phase: 0.,
                retrigger: false,
            },
        },
        mo::Source {
            id: "touch".into(),
            scope: mo::Scope::Voice,
            generator: mo::Generator::NoteRandom { seed: 20260910 },
        },
    ];
    p.modulation.routes = vec![
        route("bloom", target("colour", fx::Control::Gain), 0.32),
        route("mouth", target("vowel", fx::Control::Hz), 2400.),
        route(
            "grain",
            mo::Target::HarmonicAmp { first: 9, last: 32 },
            0.009,
        ),
        route(
            "breath",
            mo::Target::HarmonicPhase { first: 3, last: 16 },
            35.,
        ),
        route("pulse", target("vowel", fx::Control::Hz), 170.),
        route(
            "touch",
            mo::Target::PitchSemitones,
            0.015,
        ),
    ];
    p
}

struct Score {
    stage: Stage,
    commands: Vec<String>,
    statuses: Vec<String>,
}
impl Score {
    fn cmd(&mut self, command: impl Into<String>) {
        let command = command.into();
        assert!(command.len() < 4096, "oversize command");
        if !self.stage.apply_timeline_command(&command) {
            panic!(
                "preflight command {}: {}\n{}",
                self.commands.len() + 1,
                command,
                self.stage.status_line()
            );
        }
        self.statuses.push(self.stage.status_line());
        self.commands.push(command);
    }
    fn clip(&mut self, track: usize, section: usize) {
        self.cmd(format!("go clip #{}", track * 8 + section + 1));
    }
    fn values(&mut self, kind: DeviceKind, values: &[(u32, f32)]) {
        self.cmd(format!(
            "param {}",
            values
                .iter()
                .map(|(id, v)| {
                    let def = kind.spec().params.iter().find(|d| d.id == *id).unwrap();
                    assert!(
                        *v >= def.min && *v <= def.max,
                        "{}={} outside {}..{}",
                        def.name,
                        v,
                        def.min,
                        def.max
                    );
                    format!("machine.{}={v}", def.name.replace(' ', "_"))
                })
                .collect::<Vec<_>>()
                .join(";")
        ));
    }
    fn rhythm(
        &mut self,
        division: usize,
        offsets: &[usize],
        gate: usize,
        pitches: &[i32],
        velocities: &[u8],
    ) {
        self.cmd(format!(
            "rhythm {division} {} gate {gate} pitch {} vel {}",
            csv(offsets),
            csv(pitches),
            csv(velocities)
        ));
    }
    fn lock(&mut self, param: &str, value: impl ToString, ticks: &[usize]) {
        if !ticks.is_empty() {
            self.cmd(format!(
                "lock {param} {} at {}",
                value.to_string(),
                csv(ticks)
            ));
        }
    }
    fn sweep(&mut self, param: &str, a: impl ToString, b: impl ToString, from: usize, to: usize) {
        self.cmd(format!(
            "sweep {param} {}:{} at {from}:{to}",
            a.to_string(),
            b.to_string()
        ));
    }
}

fn empty_template() -> Song {
    let mut song = Song::default();
    song.patterns.clear();
    song.tracks[0].blocks.clear();
    song.tracks[0].machine = None;
    song.set_base_bpm(108.);
    song.desk_personality.noise_enabled = false;
    // Empty routing/arrangement scaffold only. Every sound edit, note and lock is
    // authored below through the same commands subsequently executed by DRIVE.
    for track in 0..8 {
        if track != 0 {
            song.add_track(TrackKind::Instrument);
        }
        song.rename_track(track, NAMES[track]);
        song.add_device(track, KINDS[track]).unwrap();
        if track == 3 {
            song.tracks[track].machine.as_mut().unwrap().sample = Some(SAMPLE.into());
        }
        for section in 0..8 {
            let id = PatternId((track * 8 + section + 1) as u64);
            let mut pattern =
                Pattern::empty(id, format!("{} · {}", NAMES[track], SECTIONS[section]));
            pattern.tag = format!("{}{}", (b'a' + track as u8) as char, section);
            pattern.extend_timeline(SECTION).unwrap();
            song.patterns.push(pattern);
            song.place_block(track, id, section * SECTION, SECTION)
                .unwrap();
        }
    }
    song.locators = SECTIONS
        .iter()
        .enumerate()
        .map(|(i, name)| Locator {
            tick: i * SECTION,
            name: name.to_string(),
        })
        .collect();
    song.furnish();
    song
}

fn instruments(s: &mut Score, dir: &Path) {
    use daw::params::{clay as c, drum as h, glass as g, poly as p, sampler as a, thump as k};
    s.clip(0, 0);
    s.values(
        KINDS[0],
        &[
            (k::DECAY, 180.),
            (k::RELEASE, 35.),
            (k::CLICK, 22.),
            (k::CLICK_TIME, 3.),
            (k::BEND, 12.),
            (k::BEND_TIME, 38.),
            (k::NOISE, 0.07),
            (k::NOISE_DECAY, 3.),
            (k::LEVEL, 0.68),
        ],
    );
    s.cmd("param track.volume=0.72");
    s.clip(1, 0);
    s.values(
        KINDS[1],
        &[
            (c::MATTER, 0.65),
            (c::SIZE, 0.24),
            (c::STRIKE, 0.8),
            (c::DROP, 4.),
            (c::DAMP, 0.75),
            (c::WIRES, 0.82),
            (c::DECAY, 0.7),
            (c::RELEASE, 35.),
            (c::LEVEL, 0.5),
        ],
    );
    s.cmd("param track.volume=0.62;room.mix=7;room.size=26");
    s.clip(2, 0);
    s.cmd("param drum: model=AIR HAT;decay=52ms;open decay=440ms;sweep=0st;tone=0.57;noise=0.04;crack=0.16;drive=0.015;attack=0.15ms;level=0.5");
    s.cmd("param track.volume=0.48;track.pan=10%");
    // Keep the broad-band hat's filter safely above the pitched low drum range.
    s.values(KINDS[2], &[(h::CUTOFF, 12500.)]);
    s.clip(3, 0);
    s.values(
        KINDS[3],
        &[
            (a::MODE, 2.),
            (a::SLICES, 16.),
            (a::SLICE_SOURCE, 0.),
            (a::CHOKE, 1.),
            (a::FADE_IN, 1.),
            (a::FADE_OUT, 8.),
            (a::AMP_A, 0.5),
            (a::AMP_D, 130.),
            (a::AMP_S, 0.),
            (a::AMP_R, 25.),
            (a::CUTOFF, 6200.),
            (a::GAIN, 0.55),
        ],
    );
    s.cmd("param track.volume=0.44;track.pan=-18%;echo.sync=1/8d;echo.feedback=24%;echo.tone=3100Hz;echo.mix=12%;room.mix=12%");
    s.clip(4, 0);
    s.cmd(format!(
        "spectral load {}",
        dir.join("black-current.patch.ron").display()
    ));
    s.cmd("spectral bins all amp 0");
    s.cmd("spectral bins 1 amp 0.9");
    s.cmd("spectral bins 2:8 amp 0.075:0.015");
    s.cmd("spectral bins 9:32 amp 0.008:0.001");
    s.cmd("spectral bins 3:32 phase 0:155");
    s.cmd("param spectral: attack=7ms;decay=600ms;sustain=0.9;release=150ms;level=0.6;mono=legato;glide=85ms;macro 1=0;macro 2=0.1;macro 3=0");
    s.cmd("param track.volume=0.70;track.pan=0");
    s.clip(5, 0);
    s.values(
        KINDS[5],
        &[
            (g::ATTACK, 9.),
            (g::DECAY, 1400.),
            (g::SUSTAIN, 0.18),
            (g::RELEASE, 1000.),
            (g::LEVEL, 0.38),
            (g::RATIO, 1.),
            (g::INDEX, 0.9),
            (g::I_DECAY, 360.),
            (g::I_SUSTAIN, 0.08),
            (g::RATIO_B, 2.),
            (g::INDEX_B, 0.35),
            (g::CASCADE, 0.05),
            (g::BODY, 0.7),
            (g::SUB, 0.),
            (g::WIDTH, 0.35),
            (g::CUTOFF, 5200.),
            (g::RESONANCE, 0.707),
            (g::VEL_INDEX, 0.25),
        ],
    );
    s.cmd("param track.volume=0.62;track.pan=-12%;echo.sync=1/8d;echo.feedback=32%;echo.tone=3700Hz;echo.mix=20%;echo.ping-pong=on;room.algo=hall;room.predelay=24ms;room.size=66%;room.damp=62%;room.mix=27%");
    s.clip(6, 0);
    s.values(
        KINDS[6],
        &[
            (p::A_WAVE, 1.),
            (p::B_WAVE, 2.),
            (p::A_LEVEL, 65.),
            (p::B_LEVEL, 22.),
            (p::A_FINE, -3.),
            (p::B_FINE, 3.),
            (p::F_CUTOFF, 1400.),
            (p::F_RES, 0.707),
            (p::F_ENV, 8.),
            (p::AMP_A, 1150.),
            (p::AMP_D, 2000.),
            (p::AMP_S, 82.),
            (p::AMP_R, 1800.),
            (p::GAIN, 0.23),
            (p::V_UNISON, 0.),
            (p::V_SPREAD, 70.),
        ],
    );
    s.cmd("param track.volume=0.60;room.algo=hall;room.predelay=30ms;room.size=78%;room.damp=69%;room.mix=32%;echo.sync=1/4;echo.feedback=24%;echo.tone=2300Hz;echo.mix=11%");
    s.clip(7, 0);
    s.cmd("param acid: wave=saw;cutoff=2100Hz;resonance=0.07;env mod=0.035;decay=850ms;accent=0;glide=90ms;drive=0;level=0.22;vibrato speed=5.1Hz;vibrato intensity=0ct;ornament=off");
    s.cmd("param track.volume=0.62;track.pan=16%;echo.sync=1/8d;echo.feedback=34%;echo.tone=2700Hz;echo.mix=24%;echo.ping-pong=on;room.algo=hall;room.size=64%;room.damp=70%;room.mix=23%");
}

fn drums(s: &mut Score, section: usize) {
    // Eighth-note 3+3+2 accents occupy ONE bar. The bass is quarter-note 3+3+2.
    let bars: Vec<usize> = match section {
        0 => vec![12, 14, 15],
        4 => vec![],
        5 => (4..16).collect(),
        7 => (0..8).collect(),
        _ => (0..16).collect(),
    };
    if bars.is_empty() {
        return;
    }
    s.clip(0, section);
    let mut kicks = Vec::new();
    for &bar in &bars {
        kicks.extend([0, 6, 10].into_iter().map(|t| bar * 16 + t));
        if bar % 4 == 3 && section != 0 {
            kicks.push(bar * 16 + 15);
        }
        if bar % 8 == 6 && section >= 2 {
            kicks.push(bar * 16 + 9);
        }
    }
    s.rhythm(16, &kicks, 1, &[-24], &[113, 94, 104, 84]);
    let soft: Vec<_> = bars
        .iter()
        .filter(|b| **b % 4 == 3)
        .map(|b| b * BAR + 180)
        .collect();
    s.lock("click", 12, &soft);
    s.clip(1, section);
    let snare_bars: Vec<_> = bars
        .iter()
        .copied()
        .filter(|b| section != 0 || *b == 15)
        .collect();
    if !snare_bars.is_empty() {
        let anchors: Vec<_> = snare_bars
            .iter()
            .flat_map(|b| [b * 16 + 4, b * 16 + 12])
            .collect();
        s.rhythm(16, &anchors, 1, &[-24], &[109, 117]);
        let ghosts: Vec<_> = snare_bars
            .iter()
            .filter(|b| **b % 2 == 1)
            .flat_map(|b| [b * 16 + 3, b * 16 + 10])
            .collect();
        if !ghosts.is_empty() {
            s.rhythm(16, &ghosts, 1, &[-24], &[36, 47]);
        }
        let backticks: Vec<_> = anchors.iter().map(|a| a * 12).collect();
        s.lock("wires", 0.82, &backticks);
        s.lock("strike", 0.8, &backticks);
        let ghostticks: Vec<_> = ghosts.iter().map(|a| a * 12).collect();
        s.lock("wires", 0.24, &ghostticks);
        s.lock("strike", 0.35, &ghostticks);
        if section == 3 || section == 6 {
            // 1/48 whole-note division: exact triplet sixteenths at four ticks.
            s.rhythm(
                48,
                &[15 * 48 + 33, 15 * 48 + 34, 15 * 48 + 35],
                1,
                &[-24],
                &[34, 48, 71],
            );
        }
    }
    s.clip(2, section);
    let mut closed = Vec::new();
    let mut open = Vec::new();
    for &b in &bars {
        let pattern: &[usize] = if b % 4 == 2 {
            &[0, 2, 5, 6, 8, 11, 12, 14]
        } else {
            &[0, 2, 4, 6, 8, 12, 14]
        };
        closed.extend(pattern.iter().map(|t| b * 16 + t));
        open.push(b * 16 + 10);
    }
    // Delay selected offbeats by one native tick (11.57ms), never anchors.
    let closed_ticks: Vec<_> = closed
        .iter()
        .map(|t| t * 12 + usize::from(t % 4 == 2))
        .collect();
    s.rhythm(
        192,
        &closed_ticks,
        12,
        &[-18],
        &[76, 43, 61, 51, 72, 50, 63],
    );
    s.rhythm(16, &open, 1, &[-14], &[67, 74, 61, 79]);
    s.lock(
        "decay",
        42,
        &bars
            .iter()
            .filter(|b| **b % 2 == 1)
            .map(|b| b * BAR)
            .collect::<Vec<_>>(),
    );
    if [2, 3, 6].contains(&section) {
        for (bar, up) in [(7, true), (15, false)] {
            let from = bar * 64 + 48;
            let to = (bar + 1) * 64;
            // Existing onset uses its source pitch; avoid octave-doubling it.
            s.cmd(format!(
                "ratchet 64 {from}:{to} spacing {} gate 1 vel {}",
                if up { "4:1" } else { "1:4" },
                if up { "35:85" } else { "81:29" }
            ));
            s.sweep(
                "cutoff",
                if up { 3200 } else { 13800 },
                if up { 13800 } else { 3200 },
                bar * BAR + 144,
                (bar + 1) * BAR,
            );
        }
        s.lock("cutoff", 12500, &[8 * BAR]);
    }
    s.clip(3, section);
    let mut hits = Vec::new();
    for &b in &bars {
        hits.extend(
            [3, 7, 11, 14]
                .into_iter()
                .filter(|t| section != 0 || *t == 14)
                .map(|t| b * 16 + t),
        );
    }
    s.rhythm(16, &hits, 1, &[0, 5, -2, 7], &[68, 84, 58, 77]);
    for slice in 0..4 {
        let times: Vec<_> = hits
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 4 == slice)
            .map(|(_, t)| t * 12)
            .collect();
        s.lock("slice", [2, 6, 11, 15][slice], &times);
    }
    if [2, 3, 6].contains(&section) {
        for b in [7, 15] {
            s.cmd(format!(
                "ratchet 64 {}:{} spacing 3:1 gate 1 vel 72:35",
                b * 64 + 56,
                (b + 1) * 64
            ));
            s.lock("reverse", "on", &[b * BAR + 168]);
            s.sweep("cutoff", 7800, 900, b * BAR + 144, (b + 1) * BAR);
        }
        s.lock("reverse", "off", &[8 * BAR]);
        s.lock("cutoff", 6200, &[8 * BAR]);
        // A reversing contour: open, then close across the final four beats.
        s.sweep("echo.mix", 12, 34, 15 * BAR, 15 * BAR + 96);
        s.sweep("echo.mix", 34, 12, 15 * BAR + 96, 16 * BAR);
    }
}

fn harmony(section: usize, cycle: usize) -> (i32, [i32; 4], [i32; 5]) {
    // Root, rootless four-note pad, five-note melodic cell. All MIDI numbers.
    if section == 3 && cycle >= 4 {
        return match cycle % 2 {
            0 => (40, [55, 59, 64, 77], [76, 77, 79, 74, 71]),
            _ => (41, [57, 60, 65, 76], [77, 79, 76, 74, 72]),
        };
    }
    if section == 4 && cycle >= 2 && cycle < 6 {
        return match cycle {
            2 => (38, [56, 60, 66, 74], [74, 76, 78, 80, 82]),
            3 => (38, [58, 62, 68, 76], [80, 78, 76, 74, 72]),
            4 => (38, [57, 61, 67, 75], [75, 77, 79, 81, 83]),
            _ => (43, [53, 57, 60, 76], [76, 74, 72, 71, 69]),
        };
    }
    if section == 7 && cycle >= 6 {
        return (41, [57, 64, 71, 79], [81, 83, 88, 86, 84]);
    }
    match cycle % 4 {
        0 => (41, [57, 64, 71, 79], [69, 71, 76, 74, 72]),
        1 => (40, [55, 62, 66, 71], [67, 69, 74, 71, 66]),
        2 => (38, [53, 60, 64, 69], [65, 67, 72, 69, 64]),
        _ => (43, [53, 60, 64, 69], [69, 71, 76, 74, 72]),
    }
}

fn melodic(s: &mut Score, section: usize) {
    s.clip(4, section);
    let begin = if section == 0 { 4 } else { 0 };
    for cycle in begin..8 {
        let (root, _, _) = harmony(section, cycle);
        let root = root - 12; // D1..G1 foundations, with deliberate octave replies.
        let pitch = if section == 4 || (section == 7 && cycle >= 4) {
            vec![root - 60]
        } else {
            vec![root - 60, root - 60, root - 48]
        };
        if pitch.len() == 1 {
            s.rhythm(4, &[cycle * 8], 8, &pitch, &[69]);
        } else {
            s.cmd(format!(
                "group 4 3,3,2 at {} pitch {} vel 96,86,91",
                cycle * 8,
                csv(pitch)
            ));
        }
    }
    s.cmd("legato 2");
    let (a, b) = match section {
        0 => (0.02, 0.08),
        1 => (0.08, 0.20),
        2 => (0.18, 0.48),
        3 => (0.40, 0.80),
        4 => (0.04, 0.10),
        5 => (0.14, 0.45),
        6 => (0.40, 0.93),
        _ => (0.28, 0.0),
    };
    s.sweep("macro_1", a, b, 0, SECTION);
    s.sweep("macro_2", b, a, 0, SECTION);
    s.sweep("macro_3", a * 0.7, b * 0.7, 0, SECTION);
    if [2, 3, 6].contains(&section) {
        s.sweep("h009_phase", 15, 170, 7 * BAR, 8 * BAR);
        s.sweep("h005_amp", 0.025, 0.13, 15 * BAR, 16 * BAR);
    }
    s.clip(5, section);
    for cycle in 0..8 {
        let (_, _, mut notes) = harmony(section, cycle);
        let base = cycle * 32;
        if section == 0 && cycle < 2 {
            s.rhythm(
                16,
                &[base, base + 6, base + 12, base + 20, base + 26],
                3,
                &notes.map(|n| n - 60),
                &[69, 58, 75, 61, 54],
            );
        } else {
            if section == 2 {
                notes.reverse();
            }
            if section == 5 {
                notes.rotate_left(1);
            }
            if section == 6 && cycle % 2 == 1 {
                notes = notes.map(|n| n + 12);
            }
            let offsets = if cycle % 2 == 0 {
                [0, 6, 14, 20, 27]
            } else {
                [2, 8, 13, 22, 28]
            };
            let vel = if section == 7 {
                [62, 52, 57, 46, 39]
            } else {
                [78, 59, 73, 63, 54]
            };
            if section == 7 && cycle == 7 {
                s.rhythm(16, &[base, base + 8], 3, &[21, 23], &[45, 33]);
            } else {
                s.rhythm(
                    16,
                    &offsets.map(|t| base + t),
                    3,
                    &notes.map(|n| n - 60),
                    &vel,
                );
            }
            if [2, 3, 6].contains(&section) && cycle % 4 == 3 {
                let chord = harmony(section, cycle).1;
                s.rhythm(
                    32,
                    &[base * 2 + 57, base * 2 + 59, base * 2 + 61, base * 2 + 63],
                    1,
                    &chord.map(|n| n - 48),
                    &[51, 63, 56, 40],
                );
            }
        }
    }
    s.sweep(
        "index",
        if section == 3 { 1.5 } else { 0.7 },
        if section == 7 { 0.4 } else { 1.05 },
        0,
        SECTION,
    );
    s.clip(6, section);
    let mut bases = Vec::new();
    let mut stacks = Vec::new();
    for cycle in 0..8 {
        let chord = harmony(section, cycle).1;
        bases.push(chord[0] - 60);
        stacks.push(csv(chord.map(|n| n - chord[0])));
    }
    s.rhythm(
        4,
        &[0, 8, 16, 24, 32, 40, 48, 56],
        8,
        &bases,
        &[61, 57, 60, 55, 64, 59, 56, 51],
    );
    s.cmd(format!("voice {}", stacks.join("|")));
    // Four held notes + four tails. Release 1.8s < 4.444s between chords.
    for cycle in 0..4 {
        let from = cycle * 4 * BAR;
        let mid = from + 2 * BAR;
        let end = from + 4 * BAR;
        s.sweep(
            "machine.filter_cutoff",
            if section == 7 { 950 } else { 1150 },
            if section == 4 { 2600 } else { 1950 },
            from,
            mid,
        );
        s.sweep(
            "machine.filter_cutoff",
            if section == 4 { 2600 } else { 1950 },
            if section == 7 { 650 } else { 1150 },
            mid,
            end,
        );
    }
    // The export dialog currently caps tail at 10s. Two silent padding bars
    // after the 128 musical bars yield a 14.444s natural release in the export.
    if section == 7 {
        s.cmd(format!("length {}", SECTION + 2 * BAR));
    }
    s.clip(7, section);
    let start = if section == 0 { 2 } else { 0 };
    let mut expression = Vec::new();
    for cycle in start..8 {
        let (_, chord, _) = harmony(section, cycle);
        let base = cycle * 32;
        if section == 7 && cycle >= 6 {
            continue;
        }
        let offsets = [base + 10, base + 17, base + 24];
        let pitches = [chord[3] - 60, chord[2] - 60, chord[1] - 60];
        s.rhythm(16, &offsets, 5, &pitches, &[62, 70, 58]);
        // A rest between responses is explicit; do not apply global legato,
        // which would erase that space. Overlap the first two gates locally.
        s.rhythm(16, &[base + 10], 8, &pitches[..1], &[62]);
        s.rhythm(16, &[base + 17], 8, &pitches[1..2], &[70]);
        expression.push((base + 24) * 12);
    }
    let ornaments = ["kan-swar", "meend", "andolan", "khatka", "murki", "gamak"];
    for (i, &tick) in expression.iter().enumerate() {
        if i % 2 == 1 || section == 4 {
            let which = (i + section) % 6;
            s.lock("ornament", ornaments[which], &[tick]);
            s.lock(
                "ornament_time",
                [75, 260, 1000, 130, 150, 400][which],
                &[tick],
            );
            s.lock(
                "ornament_from",
                [-100, -200, -35, -100, -200, -90][which],
                &[tick],
            );
            s.lock("ornament_other", [100, 0, 18, 200, 100, 80][which], &[tick]);
            s.lock(
                "ornament_speed",
                if which == 2 { 0.85 } else { 4.5 },
                &[tick],
            );
            // Locks are scoped to the decorated onset, not a whole phrase.
            s.lock("ornament", "off", &[tick + 12]);
            if which == 1 || which == 2 {
                s.sweep(
                    "vibrato_intensity",
                    0,
                    if section == 6 { 22 } else { 12 },
                    tick,
                    tick + 48,
                );
                s.lock("vibrato_intensity", 0, &[tick + 60]);
            }
        }
    }
    if section == 7 {
        s.sweep("cutoff", 1700, 900, 0, SECTION);
    }
}

fn render(song: &Song, dir: &Path) -> Result<()> {
    let (spec, _) = daw::song_graph::build_song(song);
    for (name, start, bars) in [
        ("groove", 16., 8.),
        ("peak-transition", 104., 12.),
        ("breakdown-return", 72., 16.),
    ] {
        println!("Rendering {name}");
        bounce(
            &spec,
            &BounceOptions {
                sample_rate: 48000,
                block_frames: 256,
                bpm: 108.,
                length_beats: (start + bars) * 4.,
                start_beats: start * 4.,
                format: BounceFormat::Float32,
            },
            &dir.join(format!("{name}.wav")),
        )?;
        assert!(std::fs::metadata(dir.join(format!("{name}.wav")))?.len() > 100_000);
    }
    Ok(())
}

fn audit(song: &Song) -> Result<()> {
    use daw::params::{acid, poly};
    assert_eq!(song.tracks.len(), 8);
    assert_eq!(song.bpm, 108.);
    assert_eq!(song.end_tick(), 130 * BAR);
    assert!(song.visuals.is_none());
    let mut total = 0;
    for (track, name) in NAMES.iter().enumerate() {
        let mut notes = 0;
        for block in &song.tracks[track].blocks {
            let p = song.pattern(block.pattern_id).unwrap();
            for step in 0..p.step_count() {
                for n in &p.trig(step).notes {
                    let tick = step*12 + n.micro_ticks as usize;
                    assert!(block.start_tick+tick+n.length_ticks <= 128*BAR);
                    notes += 1;
                }
            }
        }
        assert!(notes>0);
        total+=notes;
        println!("{name}: {notes} editable notes");
    }
    for section in [1,2,3,6] {
        let p=song.pattern(PatternId(9+section)).unwrap();
        for bar in 0..16 {for beat in [1,3] {assert!(p.trig(bar*16+beat*4).notes.iter().any(|n|n.velocity>=100 && n.micro_ticks==0));}}
    }
    let bass=song.pattern(PatternId(34)).unwrap();
    for cycle in 0..8 {
        for (beat,gate) in [(0,146),(3,146),(6,98)] {
            let n=&bass.trig(cycle*32+beat*4).notes[0];
            assert_eq!(n.length_ticks,if cycle==7 && beat==6 {96}else{gate});
        }
    }
    let pad=song.tracks[6].machine.as_ref().unwrap();
    assert_eq!(pad.value(poly::V_UNISON),0.);
    assert!(pad.value(poly::AMP_R) < (8.*60./108.*1000.) as f32);
    for section in 0..8 {
        let p=song.pattern(PatternId(49+section)).unwrap();
        for cycle in 0..8 {assert_eq!(p.trig(cycle*32).notes.len(),4);}
    }
    let mut ornaments=std::collections::BTreeSet::new();
    for p in song.patterns.iter().filter(|p|p.id.0>=57) {
        for step in 0..p.step_count() {
            if let Some(v)=p.trig(step).lock(acid::ORNAMENT) {ornaments.insert(v as u32);}
        }
    }
    assert_eq!(ornaments,(0..=6).collect());
    println!("AUDIT PASS: {total} notes; backbeats retained; 3+3+2 quarter-beat bass with 2-tick overlaps; four-note pad plus four releases; all six Acid ornaments; 128 musical + 2 silent tail bars; visuals absent.");
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--inventory") {
        for kind in KINDS {
            println!("{kind:?}");
            for d in kind.spec().params {
                println!(
                    "{} {} {}..{} default {}",
                    d.id, d.name, d.min, d.max, d.default
                );
            }
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "--render") {
        let mut stage = Stage::new();
        stage.open(&args[2])?;
        return render(stage.song(), Path::new(&args[3]));
    }
    if args.get(1).is_some_and(|s|s=="--audit") {
        let mut stage=Stage::new();stage.open(&args[2])?;
        audit(stage.song())?;
        if let Some(other)=args.get(3) {
            let mut recalled=Stage::new();recalled.open(other)?;
            assert!(stage.song()==recalled.song(),"native save must match preflight");
            println!("Native saved song equals preflight exactly.");
        }
        return Ok(());
    }
    let dir = PathBuf::from(args.get(1).ok_or("provide a fresh output directory")?);
    std::fs::create_dir(&dir)?;
    assert!(Path::new(SAMPLE).is_file());
    let patch = patch();
    daw::spectral::validate(&patch)?;
    write_new(
        &dir.join("black-current.patch.ron"),
        &ron::ser::to_string_pretty(&patch, ron::ser::PrettyConfig::default())?,
    )?;
    let mut stage = Stage::new();
    *stage.song_mut() = empty_template();
    let project = dir.join("Lagoon of Broken Glass.stage.ron");
    stage.save_as(&project)?;
    stage.save_as(dir.join("empty-template.stage.ron"))?;
    stage.open(&project)?;
    stage.set_palette_open(false);
    let mut score = Score {
        stage,
        commands: Vec::new(),
        statuses: Vec::new(),
    };
    instruments(&mut score, &dir);
    for section in 0..8 {
        println!("Preflight {}", SECTIONS[section]);
        drums(&mut score, section);
        melodic(&mut score, section);
    }
    score.cmd("render-config format wav24 rate 48000 tail 10 range song");
    let mut drive = String::from(
        "# Lagoon of Broken Glass: eight-track native composition TAKE\n# Empty instrument/arrangement scaffold; no imported musical notes.\npace 1\ntimeout 900\nuntil library ready\n",
    );
    for c in &score.commands {
        drive.push_str(&format!("key ctrl+shift+p\ntext {c}\nkey Enter\n"));
    }
    drive.push_str("key Escape\nkey ctrl+s\nkey Home\nkey Space\nkey ctrl+shift+r\nuntil play bar 017\nkey ArrowDown\nkey ArrowRight\nkey Enter\n");
    for bar in [33,49,65,81,97,113] {
        drive.push_str(&format!("until play bar {bar:03}\necho AUDITION SECTION {bar}\n"));
    }
    drive.push_str(&format!("shot {}\n",dir.join("tracker-outro.png").display()));
    drive.push_str("until play bar 130\nkey Space\nkey Escape\n");
    drive.push_str("key Home\nkey ctrl+shift+e\nkey ArrowDown\nkey Enter\nkey ctrl+a\n");
    drive.push_str(&format!(
        "text {}\n",
        FULL_RENDER
    ));
    drive.push_str("key Enter\nkey ArrowDown\nkey ArrowDown\nkey ArrowDown\nkey ArrowDown\nkey Enter\nuntil EXPORT COMPLETE\nkey Escape\nkey Home\nkey Space\nkey ctrl+shift+r\nuntil play bar 002\n");
    drive.push_str(&format!("shot {}\n", dir.join("tracker.png").display()));
    drive.push_str("echo TAKE COMPLETE\n");
    write_new(&dir.join("lagoon-v1.drive"), &drive)?;
    write_new(
        &dir.join("preflight-commands.txt"),
        &score.commands.join("\n"),
    )?;
    write_new(
        &dir.join("preflight-receipts.txt"),
        &score.statuses.join("\n"),
    )?;
    score.stage.save_as(dir.join("preflight.stage.ron"))?;
    let notes: usize = score
        .stage
        .song()
        .patterns
        .iter()
        .map(|p| {
            (0..p.step_count())
                .map(|i| p.trig(i).notes.len())
                .sum::<usize>()
        })
        .sum();
    let (spec, _) = daw::song_graph::build_song(score.stage.song());
    let _schedule = spec.compile_at_tempo(48000, 256, 108.)?;
    println!(
        "PREFLIGHT OK: {} native commands, {notes} notes, 8 tracks, 128 bars. Empty native starting project: {}",
        score.commands.len(),
        project.display()
    );
    Ok(())
}
