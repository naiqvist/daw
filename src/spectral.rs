//! Green-side spectral patch authoring shared by the native palette and tests.
//! No UI-only state and no agent-only interface. Every edit preflights a copy.
use crate::audio::{
    spectral::{SpectralPatch, SpectralVoices},
    spectral_fx as fx, spectral_mod as mo,
};
use crate::{devices::DeviceKind, params::spectral as p, sequencing::Device};

pub const WORKSPACE_BYTES: usize = 64 * 1024 * 1024;
pub fn validate(patch: &SpectralPatch) -> Result<(), String> {
    // Validate at the highest supported rate so a saved patch is also admitted
    // by lower-rate playback/export. The audio compiler validates again at its rate.
    SpectralVoices::prepare(192_000.0, 256, patch, WORKSPACE_BYTES).map(|_| ())
}
fn number(text: &str) -> Result<f32, String> {
    text.parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("expected finite number, got {text}"))
}
fn control(text: &str) -> Result<fx::Control, String> {
    Ok(match text {
        "gain" => fx::Control::Gain,
        "hz" | "cutoff" => fx::Control::Hz,
        "q" => fx::Control::Q,
        "drive" => fx::Control::Drive,
        "bias" => fx::Control::Bias,
        "mix" => fx::Control::Mix,
        "feedback" => fx::Control::Feedback,
        _ => return Err("FX control: gain, hz, q, drive, bias, mix, feedback".into()),
    })
}
fn range(text: &str) -> Result<(usize, usize), String> {
    let (a, b) = text.split_once(':').unwrap_or((text, text));
    let a = a.parse::<usize>().map_err(|_| "invalid harmonic")?;
    let b = b.parse::<usize>().map_err(|_| "invalid harmonic")?;
    if a == 0 || a > b || b > 128 {
        return Err("harmonics must be in 1..128".into());
    }
    Ok((a, b))
}
fn target(text: &str) -> Result<mo::Target, String> {
    Ok(match text {
        "pitch" => mo::Target::PitchSemitones,
        "amp" => mo::Target::Amplitude,
        "shift" => mo::Target::ShiftBins,
        _ => {
            if let Some((node, param)) = text.strip_prefix("fx.").and_then(|s| s.split_once('.')) {
                mo::Target::Fx {
                    node: node.into(),
                    control: control(param)?,
                }
            } else if let Some((bins, field)) =
                text.strip_prefix("h").and_then(|s| s.split_once('.'))
            {
                let (first, last) = range(bins)?;
                match field {
                    "amp" => mo::Target::HarmonicAmp { first, last },
                    "phase" => mo::Target::HarmonicPhase { first, last },
                    _ => return Err("harmonic target ends in .amp or .phase".into()),
                }
            } else {
                return Err(
                    "target: pitch, amp, shift, h1:16.amp, h1:16.phase or fx.<name>.<control>"
                        .into(),
                );
            }
        }
    })
}
fn new_module(kind: &str) -> Result<fx::Module, String> {
    Ok(match kind {
        "gain" => fx::Module::Gain { gain: 1.0 },
        "filter" => fx::Module::Filter {
            mode: fx::FilterMode::Lowpass,
            hz: 3000.0,
            q: 0.707,
        },
        "onepole" => fx::Module::OnePole {
            highpass: false,
            hz: 3000.0,
        },
        "shape" => fx::Module::Shape {
            mode: fx::ShapeMode::Soft,
            drive: 2.0,
            bias: 0.0,
            mix: 1.0,
        },
        "delay" => fx::Module::Delay {
            ms: 250.0,
            feedback: 0.3,
            damp_hz: 6000.0,
        },
        "disperser" => fx::Module::Disperser {
            hz: 1000.0,
            q: 0.7,
            stages: 4,
        },
        "tilt" => fx::Module::Tilt {
            hz: 1000.0,
            db: 0.0,
        },
        "dc" => fx::Module::DcBlock,
        "reverb" => fx::Module::Reverb {
            size: 0.6,
            decay: 0.5,
            damp: 0.5,
        },
        _ => {
            return Err(
                "module: gain, filter, onepole, shape, delay, disperser, tilt, dc, reverb".into(),
            );
        }
    })
}
fn set_fx(module: &mut fx::Module, field: &str, text: &str) -> Result<(), String> {
    if let fx::Module::Filter { mode, .. } = module {
        if field == "mode" {
            *mode = match text {
                "lp" => fx::FilterMode::Lowpass,
                "hp" => fx::FilterMode::Highpass,
                "bp" => fx::FilterMode::Bandpass,
                "notch" => fx::FilterMode::Notch,
                "peak" => fx::FilterMode::Peak,
                "allpass" => fx::FilterMode::Allpass,
                _ => return Err("filter mode: lp, hp, bp, notch, peak, allpass".into()),
            };
            return Ok(());
        }
    }
    if let fx::Module::Shape { mode, .. } = module {
        if field == "mode" {
            *mode = match text {
                "clip" => fx::ShapeMode::Clip,
                "soft" => fx::ShapeMode::Soft,
                "cubic" => fx::ShapeMode::Cubic,
                "fold" => fx::ShapeMode::Fold,
                "crush" => fx::ShapeMode::Crush,
                _ => return Err("shape mode: clip, soft, cubic, fold, crush".into()),
            };
            return Ok(());
        }
    }
    let v = number(text)?;
    match (module, field) {
        (fx::Module::Gain { gain }, "gain") => *gain = v,
        (
            fx::Module::Filter { hz, .. }
            | fx::Module::OnePole { hz, .. }
            | fx::Module::Disperser { hz, .. }
            | fx::Module::Tilt { hz, .. },
            "hz",
        ) => *hz = v,
        (fx::Module::Filter { q, .. } | fx::Module::Disperser { q, .. }, "q") => *q = v,
        (fx::Module::Shape { drive, .. }, "drive") => *drive = v,
        (fx::Module::Shape { bias, .. }, "bias") => *bias = v,
        (fx::Module::Shape { mix, .. }, "mix") => *mix = v,
        (fx::Module::Delay { ms, .. }, "ms") => *ms = v,
        (fx::Module::Delay { feedback, .. }, "feedback") => *feedback = v,
        (fx::Module::Delay { damp_hz, .. }, "damp_hz") => *damp_hz = v,
        (fx::Module::Disperser { stages, .. }, "stages")
            if v >= 0.0 && v <= 32.0 && v.fract() == 0.0 =>
        {
            *stages = v as u32
        }
        (fx::Module::Tilt { db, .. }, "db") => *db = v,
        (fx::Module::Reverb { size, .. }, "size") => *size = v,
        (fx::Module::Reverb { decay, .. }, "decay") => *decay = v,
        (fx::Module::Reverb { damp, .. }, "damp") => *damp = v,
        (fx::Module::OnePole { highpass, .. }, "highpass") if v == 0.0 || v == 1.0 => {
            *highpass = v == 1.0
        }
        _ => {
            return Err(format!(
                "unsupported module field {field}; use spectral inspect"
            ));
        }
    }
    Ok(())
}
fn set_mod(source: &mut mo::Source, field: &str, text: &str) -> Result<(), String> {
    if let mo::Generator::NoteRandom { seed } = &mut source.generator {
        if field == "seed" {
            *seed = text
                .parse()
                .map_err(|_| "seed must be an unsigned integer")?;
            return Ok(());
        }
    }
    if let mo::Generator::Lfo { shape, .. } = &mut source.generator {
        if field == "shape" {
            *shape = match text {
                "sine" => mo::Shape::Sine,
                "triangle" => mo::Shape::Triangle,
                "up" => mo::Shape::SawUp,
                "down" => mo::Shape::SawDown,
                "square" => mo::Shape::Square,
                _ => return Err("LFO shape: sine, triangle, up, down, square".into()),
            };
            return Ok(());
        }
    }
    let v = number(text)?;
    match (&mut source.generator, field) {
        (mo::Generator::Lfo { hz, .. }, "hz") => *hz = v,
        (mo::Generator::Lfo { phase, .. }, "phase") => *phase = v,
        (mo::Generator::Lfo { retrigger, .. }, "retrigger") if v == 0.0 || v == 1.0 => {
            *retrigger = v == 1.0
        }
        (mo::Generator::Envelope { attack_ms, .. }, "attack") => *attack_ms = v,
        (mo::Generator::Envelope { decay_ms, .. }, "decay") => *decay_ms = v,
        (mo::Generator::Envelope { sustain, .. }, "sustain") => *sustain = v,
        (mo::Generator::Envelope { release_ms, .. }, "release") => *release_ms = v,
        (mo::Generator::Macro { index }, "index")
            if v.fract() == 0.0 && (1.0..=8.0).contains(&v) =>
        {
            *index = v as u8
        }
        _ => {
            return Err(format!(
                "unsupported modulator field {field}; use spectral inspect"
            ));
        }
    }
    Ok(())
}

/// Read/export do not mutate the device. Edit commands replace it only after
/// the full patch validates. The stage groups that replacement into one undo.
pub fn command(device: &mut Device, words: &[&str]) -> Result<String, String> {
    if device.kind != DeviceKind::Spectral {
        return Err("select a spectral instrument first".into());
    }
    let mut patch = SpectralPatch::from_device(device);
    match words {
        ["inspect"]|[]=>return Ok(format!("FX {:?}; connections {:?}; modulators {:?}; mod routes {:?}",patch.fx.nodes,patch.fx.routes,patch.modulation.sources,patch.modulation.routes)),
        ["load",path @ ..] if !path.is_empty()=>{
            let text=std::fs::read_to_string(path.join(" ")).map_err(|e|e.to_string())?;
            patch=ron::from_str(&text).map_err(|e|format!("invalid spectral patch: {e}"))?;
        }
        ["save",path @ ..] if !path.is_empty()=>{
            use std::io::Write;
            let text=ron::ser::to_string_pretty(&patch,ron::ser::PrettyConfig::default()).map_err(|e|e.to_string())?;
            let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(path.join(" ")).map_err(|e|e.to_string())?;
            file.write_all(text.as_bytes()).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
            return Ok("patch saved (new file)".into());
        }
        ["bins",selection,field,values]=>{
            let bins:Vec<usize>=match *selection {"all"=>(1..=128).collect(),"odd"=>(1..=128).step_by(2).collect(),
                "even"=>(2..=128).step_by(2).collect(),other=>{let(a,b)=range(other)?;(a..=b).collect()}};
            let(a,b)=values.split_once(':').unwrap_or((values,values));let a=number(a)?;let b=number(b)?;
            for (i,h) in bins.iter().enumerate() {
                let value=a+(b-a)*i as f32/(bins.len()-1).max(1) as f32;
                let id=match *field {"amp"=>p::amp(*h-1),"phase"=>p::phase(*h-1),_=>return Err("bins field: amp or phase".into())};
                let row=&p::TABLE[id as usize];if !(row.min..=row.max).contains(&value) {return Err("bin values exceed parameter range".into());}patch.params.set(id,value);
            }
        }
        ["add",id,kind]=>patch.fx.nodes.push(fx::Node{id:(*id).into(),module:new_module(kind)?}),
        ["connect",from,to,gain]=>{
            let gain=number(gain)?;
            if let Some(route)=patch.fx.routes.iter_mut().find(|r|r.from==*from&&r.to==*to) {route.gain=gain;}
            else {patch.fx.routes.push(fx::Route::new(*from,*to,gain));}
        }
        ["disconnect",from,to]=>{
            let before=patch.fx.routes.len();patch.fx.routes.retain(|r|r.from!=*from||r.to!=*to);
            if patch.fx.routes.len()==before {return Err("connection not found".into());}
        }
        ["fx",id,field,value]=>{
            let node=patch.fx.nodes.iter_mut().find(|n|n.id==*id).ok_or("unknown FX module")?;set_fx(&mut node.module,field,value)?;
        }
        ["source",id,kind,scope]=>{
            let scope=match *scope {"voice"=>mo::Scope::Voice,"shared"=>mo::Scope::Shared,_=>return Err("scope: voice or shared".into())};
            let generator=match *kind {
                "lfo"=>mo::Generator::Lfo{shape:mo::Shape::Sine,hz:1.0,phase:0.0,retrigger:false},
                "envelope"=>mo::Generator::Envelope{attack_ms:100.0,decay_ms:400.0,sustain:0.5,release_ms:500.0},
                "random"=>mo::Generator::NoteRandom{seed:1},"velocity"=>mo::Generator::Velocity,
                "macro"=>mo::Generator::Macro{index:1},
                _=>return Err("source: lfo, envelope, random, velocity".into())};
            patch.modulation.sources.push(mo::Source{id:(*id).into(),scope,generator});
        }
        ["mod",id,field,value]=>{
            let source=patch.modulation.sources.iter_mut().find(|s|s.id==*id).ok_or("unknown modulator")?;
            set_mod(source,field,value)?;
        }
        ["route",source,to,depth,offset @ ..] if offset.len()<=1=>{
            patch.modulation.routes.push(mo::Route{source:(*source).into(),target:target(to)?,depth:number(depth)?,
                polarity:mo::Polarity::Native,offset:offset.first().map(|v|number(v)).transpose()?.unwrap_or(0.0)});
        }
        ["depth",index,depth]=>{
            let i=index.parse::<usize>().ok().and_then(|i|i.checked_sub(1)).ok_or("route indexes start at 1")?;
            let route=patch.modulation.routes.get_mut(i).ok_or("modulation route not found")?;route.depth=number(depth)?;
        }
        ["unroute",index]=>{
            let i=index.parse::<usize>().ok().and_then(|i|i.checked_sub(1)).ok_or("route indexes start at 1")?;
            if i>=patch.modulation.routes.len() {return Err("modulation route not found".into());}patch.modulation.routes.remove(i);
        }
        _=>return Err("spectral: load/save <path>; bins <all|odd|even|1:16> <amp|phase> <a[:b]>; add <id> <kind>; connect/disconnect; fx <id> <field> <value>; source <id> <kind> <voice|shared>; mod <id> <field> <value>; route <source> <target> <depth> [offset]; depth/unroute <index>; inspect".into())
    }
    validate(&patch)?;
    patch.install(device);
    Ok(format!(
        "linked · {} FX modules · {} audio connections · {} modulators · {} mod routes",
        patch.fx.nodes.len(),
        patch.fx.routes.len(),
        patch.modulation.sources.len(),
        patch.modulation.routes.len()
    ))
}
