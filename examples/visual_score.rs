//! An authored, reusable score for Skin / Coil / Sky, not an app preset button.
//! cargo run --features visuals --example visual_score -- INPUT.stage.ron NEW-DIR [--frames]
use daw::{sequencing::Song, visuals::*};
use std::{fs::OpenOptions, io::Write, path::Path};
#[derive(serde::Serialize, serde::Deserialize)]
struct Document {
    version: u32,
    song: Song,
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(bytes)?;
    Ok(())
}
fn ramp(layer: &mut Layer, name: &str, keys: &[(u64, f32)]) {
    layer.automation.push(Lane {
        param: name.into(),
        keys: keys
            .iter()
            .map(|&(tick, value)| Key {
                tick,
                value,
                slide: true,
            })
            .collect(),
    });
}
fn lfo(layer: &mut Layer, name: &str, rate: f64, depth: f32, phase: f64) {
    layer.modulation.push(Mod {
        param: name.into(),
        depth,
        source: Modulator::Lfo {
            cycles_per_beat: rate,
            phase,
        },
    });
}
fn pulse(layer: &mut Layer, name: &str, period: u64, depth: f32, phase: u64) {
    layer.modulation.push(Mod {
        param: name.into(),
        depth,
        source: Modulator::Pulse {
            period_ticks: period,
            decay_ticks: 10.0,
            phase_ticks: phase,
        },
    });
}
fn layer(id: &str, kind: Primitive, values: &[(usize, f32)]) -> Layer {
    let mut l = Layer::new(id.into(), kind);
    l.blend = Blend::Add;
    for &(id, v) in values {
        l.params[id] = v;
    }
    l
}
fn fade(l: &mut Layer, length: u64, amount: f32) {
    ramp(
        l,
        "opacity",
        &[
            (0, 0.0),
            (4 * 192, amount),
            (length - 4 * 192, amount),
            (length - 1, 0.0),
        ],
    );
}
fn score() -> Score {
    let mut s = Score {
        seed: 71043,
        background: [0.0; 3],
        ..Default::default()
    };
    let len = 36 * 192;
    let mut skin = Clip {
        id: "skin".into(),
        length_ticks: len,
        layers: vec![],
    };
    let mut haze = layer(
        "ember",
        Primitive::Field,
        &[
            (HUE, 0.015),
            (SATURATION, 0.93),
            (FREQUENCY, 2.0),
            (SCALE, 2.0),
            (WARP, 1.2),
            (BRIGHTNESS, 0.13),
        ],
    );
    ramp(&mut haze, "phase", &[(0, 0.0), (len - 1, 9.0)]);
    fade(&mut haze, len, 0.8);
    skin.layers.push(haze);
    for i in 0..5 {
        let mut l = layer(
            &format!("drum-{i}"),
            Primitive::Rings,
            &[
                (X, (i as f32 - 2.0) * 0.65),
                (Y, if i % 2 == 0 { -0.25 } else { 0.28 }),
                (SCALE, 0.5),
                (HUE, 0.035 + i as f32 * 0.023),
                (SATURATION, 0.83),
                (BRIGHTNESS, 0.4),
                (FREQUENCY, 4.0 + i as f32),
                (SOFTNESS, 0.09),
                (WARP, 0.4),
            ],
        );
        fade(&mut l, len, 0.65);
        ramp(
            &mut l,
            "phase",
            &[(0, 0.0), (len - 1, 38.0 + i as f32 * 6.0)],
        );
        pulse(
            &mut l,
            "scale",
            if i % 2 == 0 { 48 } else { 72 },
            0.09,
            i * 8,
        );
        pulse(
            &mut l,
            "brightness",
            if i % 2 == 0 { 48 } else { 72 },
            0.35,
            i * 8,
        );
        lfo(&mut l, "y", 0.03125, 0.13, i as f64 / 5.0);
        lfo(&mut l, "warp", 0.0625, 0.2, i as f64 / 5.0);
        skin.layers.push(l);
    }
    s.clips.push(skin);
    s.arrangement.push(Placement {
        clip: "skin".into(),
        at: 0,
        length_ticks: len,
        repeat: false,
    });
    let mut coil = Clip {
        id: "coil".into(),
        length_ticks: len,
        layers: vec![],
    };
    for i in 0..6 {
        let mut l = layer(
            &format!("filament-{i}"),
            Primitive::Ribbon,
            &[
                (Y, (i as f32 - 2.5) * 0.19),
                (HUE, 0.48 + i as f32 * 0.068),
                (BRIGHTNESS, 0.85),
                (SATURATION, 0.76),
                (FREQUENCY, 2.0 + i as f32 * 0.7),
                (SOFTNESS, 0.025),
                (WARP, 0.5),
                (SCALE, 1.3),
            ],
        );
        fade(&mut l, len, 0.7);
        ramp(
            &mut l,
            "phase",
            &[
                (0, 0.0),
                (16 * 192, 35.0),
                (24 * 192, 85.0),
                (len - 1, 130.0),
            ],
        );
        ramp(
            &mut l,
            "rotation",
            &[
                (0, -15.0),
                (16 * 192, 20.0),
                (24 * 192, -25.0),
                (len - 1, 50.0),
            ],
        );
        ramp(
            &mut l,
            "frequency",
            &[
                (0, 2.0 + i as f32 * 0.7),
                (24 * 192, 7.0 + i as f32),
                (len - 1, 3.0),
            ],
        );
        lfo(&mut l, "warp", 0.125, 0.3, i as f64 / 6.0);
        lfo(&mut l, "hue", 0.015625, 0.05, i as f64 / 6.0);
        pulse(
            &mut l,
            "softness",
            if i % 2 == 0 { 24 } else { 36 },
            0.02,
            i * 3,
        );
        coil.layers.push(l);
    }
    s.clips.push(coil);
    s.arrangement.push(Placement {
        clip: "coil".into(),
        at: 32 * 192,
        length_ticks: len,
        repeat: false,
    });
    let len = 38 * 192;
    let mut sky = Clip {
        id: "sky".into(),
        length_ticks: len,
        layers: vec![],
    };
    let mut aurora = layer(
        "aurora",
        Primitive::Field,
        &[
            (HUE, 0.58),
            (SATURATION, 0.7),
            (SCALE, 2.8),
            (BRIGHTNESS, 0.48),
            (FREQUENCY, 2.5),
            (WARP, 1.8),
        ],
    );
    ramp(&mut aurora, "phase", &[(0, 0.0), (len - 1, 12.0)]);
    ramp(
        &mut aurora,
        "hue",
        &[(0, 0.58), (16 * 192, 0.76), (32 * 192, 0.53)],
    );
    ramp(
        &mut aurora,
        "opacity",
        &[(0, 0.0), (4 * 192, 0.75), (24 * 192, 0.65), (34 * 192, 0.0)],
    );
    sky.layers.push(aurora);
    for i in 0..5 {
        let mut l = layer(
            &format!("counterlight-{i}"),
            Primitive::Ribbon,
            &[
                (Y, (i as f32 - 2.0) * 0.3),
                (HUE, 0.48 + i as f32 * 0.085),
                (BRIGHTNESS, 0.55),
                (SATURATION, 0.62),
                (FREQUENCY, 1.7 + i as f32 * 0.4),
                (SOFTNESS, 0.065),
                (WARP, 1.0),
                (SCALE, 1.6),
                (ROTATION, -10.0),
            ],
        );
        ramp(
            &mut l,
            "phase",
            &[(0, i as f32), (len - 1, 16.0 + i as f32)],
        );
        ramp(
            &mut l,
            "opacity",
            &[
                (0, 0.0),
                ((4 + i) * 192, 0.5),
                (24 * 192, 0.55),
                (36 * 192, 0.0),
            ],
        );
        lfo(&mut l, "y", 0.015625, 0.18, i as f64 / 5.0);
        lfo(&mut l, "scale", 0.0078125, 0.13, i as f64 / 5.0);
        sky.layers.push(l);
    }
    s.clips.push(sky);
    s.arrangement.push(Placement {
        clip: "sky".into(),
        at: 64 * 192,
        length_ticks: len,
        repeat: false,
    });
    s
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let input = Path::new(args.get(1).ok_or("INPUT.stage.ron NEW-DIR [--frames]")?);
    let dir = Path::new(args.get(2).ok_or("give NEW output directory")?);
    let mut doc: Document = ron::from_str(&std::fs::read_to_string(input)?)?;
    let score = score();
    let compiled = Compiled::new(&score)?;
    doc.song.visuals = Some(command::encode(&score)?);
    std::fs::create_dir(dir)?;
    write_new(
        &dir.join("skin-coil-sky.visual.ron"),
        doc.song.visuals.as_ref().unwrap().as_bytes(),
    )?;
    write_new(
        &dir.join("Skin Coil Sky Visuals.stage.ron"),
        ron::ser::to_string_pretty(&doc, ron::ser::PrettyConfig::default())?.as_bytes(),
    )?;
    println!(
        "{}: 3 clips, 18 layers, crossfades, {} beats; source audio untouched",
        dir.display(),
        compiled.end_tick() / 48
    );
    if args.iter().any(|a| a == "--frames") {
        let mut gpu = gpu::Offline::new([960, 540])?;
        println!("GPU {}", gpu.adapter);
        for bar in [4, 40, 59, 80, 95, 101] {
            let frame = compiled.frame(bar as f64 * 192.0 + 24.0, 16.0 / 9.0);
            let rgba = gpu.frame(&frame)?;
            assert_eq!(rgba, gpu.frame(&frame)?, "same-time GPU repeat");
            let mut png = std::process::Command::new("ffmpeg")
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-n",
                    "-f",
                    "rawvideo",
                    "-pixel_format",
                    "rgba",
                    "-video_size",
                    "960x540",
                    "-i",
                    "pipe:0",
                    "-frames:v",
                    "1",
                ])
                .arg(dir.join(format!("bar-{bar:03}.png")))
                .stdin(std::process::Stdio::piped())
                .spawn()?;
            png.stdin.take().ok_or("no stdin")?.write_all(&rgba)?;
            if !png.wait()?.success() {
                return Err("PNG encode failed".into());
            }
            println!(
                "bar {}: {} active layers; same-time pixels identical",
                bar + 1,
                frame.info[1]
            );
        }
    }
    Ok(())
}
