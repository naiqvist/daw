//! UI-neutral, atomic visual score transactions. All values are engine units.
use super::*;
pub fn decode(payload: Option<&str>) -> Result<Score, String> {
    match payload {
        Some(text) => ron::from_str(text).map_err(|e| e.to_string()),
        None => Ok(Score::default()),
    }
}
pub fn encode(score: &Score) -> Result<String, String> {
    ron::ser::to_string_pretty(score, ron::ser::PrettyConfig::default()).map_err(|e| e.to_string())
}
fn number(s: &str) -> Result<f32, String> {
    s.parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("expected finite number: {s}"))
}
fn ticks(s: &str) -> Result<u64, String> {
    s.parse()
        .map_err(|_| format!("expected integer ticks: {s}"))
}
fn layer<'a>(s: &'a mut Score, c: &str, l: &str) -> Result<&'a mut Layer, String> {
    s.clips
        .iter_mut()
        .find(|clip| clip.id == c)
        .ok_or("unknown clip")?
        .layers
        .iter_mut()
        .find(|layer| layer.id == l)
        .ok_or("unknown layer".into())
}
fn key(layer: &mut Layer, param: &str, tick: u64, value: f32, slide: bool) -> Result<(), String> {
    let id = super::param(param)?;
    if !valid_value(id, value) {
        return Err(format!("{param}: {}..{}", PARAMS[id].1, PARAMS[id].2));
    }
    if !layer.automation.iter().any(|l| l.param == param) {
        layer.automation.push(Lane {
            param: param.into(),
            keys: vec![],
        });
    }
    let lane = layer
        .automation
        .iter_mut()
        .find(|l| l.param == param)
        .ok_or("missing lane")?;
    let k = Key { tick, value, slide };
    if let Some(old) = lane.keys.iter_mut().find(|k| k.tick == tick) {
        *old = k;
    } else {
        lane.keys.push(k);
    }
    lane.keys.sort_by_key(|k| k.tick);
    Ok(())
}
pub fn edit(score: &Score, input: &str) -> Result<Score, String> {
    let w: Vec<_> = input.split_whitespace().collect();
    if score.locked && w.as_slice() != ["unlock"] {
        return Err("visual score is LOCKED; visual unlock before editing".into());
    }
    let mut s = score.clone();
    match w.as_slice() {
        ["new"] if s.clips.is_empty() && s.arrangement.is_empty() => s=Score::default(),
        ["new"] => return Err("visual score already exists; clear explicitly or start a new project".into()),
        ["clear"] => s=Score::default(),
        ["lock"] => s.locked=true,
        ["unlock"] => s.locked=false,
        ["seed",n] => s.seed=n.parse().map_err(|_|"seed needs u32")?,
        ["background",r,g,b] => s.background=[number(r)?,number(g)?,number(b)?],
        ["clip",id,length] => {
            if s.clips.iter().any(|c| c.id==*id) { return Err("clip id already exists".into()); }
            s.clips.push(Clip{id:(*id).into(),length_ticks:ticks(length)?,layers:vec![]});
        },
        ["layer",clip,id,kind] => {
            let kind = match *kind { "disc"=>Primitive::Disc,"rings"=>Primitive::Rings,"ribbon"=>Primitive::Ribbon,"field"=>Primitive::Field,"noise"=>Primitive::Noise,_=>return Err("primitive: disc, rings, ribbon, field, noise".into()) };
            s.clips.iter_mut().find(|c|c.id==*clip).ok_or("unknown clip")?.layers.push(Layer::new((*id).into(),kind));
        },
        ["set",c,l,assignments @ ..] if !assignments.is_empty() => {
            let assignments=assignments.join(" "); let layer=layer(&mut s,c,l)?;
            for assignment in assignments.split(';') {
                let (name,v)=assignment.split_once('=').ok_or("set uses name=value; name=value")?;
                let id=param(name.trim())?; layer.params[id]=number(v.trim())?;
            }
        },
        ["blend",c,l,mode] => layer(&mut s,c,l)?.blend=match *mode {"over"=>Blend::Over,"add"=>Blend::Add,"multiply"=>Blend::Multiply,"screen"=>Blend::Screen,_=>return Err("blend: over, add, multiply, screen".into())},
        ["key",c,l,p,t,v] => key(layer(&mut s,c,l)?,p,ticks(t)?,number(v)?,false)?,
        ["key",c,l,p,t,v,"slide"] => key(layer(&mut s,c,l)?,p,ticks(t)?,number(v)?,true)?,
        ["ramp",c,l,p,time,values] => {
            let (a,b)=time.split_once(':').ok_or("ramp time start:end ticks")?;
            let (x,y)=values.split_once(':').ok_or("ramp values from:to")?;
            let (a,b)=(ticks(a)?,ticks(b)?); if a>=b {return Err("ramp needs increasing times".into());}
            let layer=layer(&mut s,c,l)?; key(layer,p,a,number(x)?,true)?; key(layer,p,b,number(y)?,false)?;
        },
        ["lfo",c,l,p,rate,depth,phase] => {
            layer(&mut s,c,l)?.modulation.push(Mod{param:(*p).into(),depth:number(depth)?,source:Modulator::Lfo{cycles_per_beat:number(rate)? as f64,phase:number(phase)? as f64}});
        },
        ["pulse",c,l,p,period,decay,depth,phase] => {
            layer(&mut s,c,l)?.modulation.push(Mod{param:(*p).into(),depth:number(depth)?,source:Modulator::Pulse{period_ticks:ticks(period)?,decay_ticks:number(decay)? as f64,phase_ticks:ticks(phase)?}});
        },
        ["random",c,l,p,step,depth,seed] => {
            layer(&mut s,c,l)?.modulation.push(Mod{param:(*p).into(),depth:number(depth)?,source:Modulator::Random{step_ticks:ticks(step)?,seed:seed.parse().map_err(|_|"seed needs u32")?}});
        },
        ["unmod",c,l,index] => {
            let layer=layer(&mut s,c,l)?; let i=ticks(index)?;
            if i==0 || i>layer.modulation.len() as u64 {return Err("route index is 1-based".into());}
            layer.modulation.remove(i as usize-1);
        },
        ["place",clip,at,length,mode] => {
            let repeat=match *mode {"once"=>false,"repeat"=>true,_=>return Err("place mode: once or repeat".into())};
            s.arrangement.push(Placement{clip:(*clip).into(),at:ticks(at)?,length_ticks:ticks(length)?,repeat});
        },
        ["unplace",index] => {
            let i=ticks(index)?; if i==0 || i>s.arrangement.len() as u64 {return Err("placement index is 1-based".into());}
            s.arrangement.remove(i as usize-1);
        },
        ["load",path @ ..] if !path.is_empty() => {
            let path=path.join(" ");
            if std::fs::metadata(&path).map_err(|e|e.to_string())?.len()>4*1024*1024 {return Err("visual score file exceeds 4 MiB".into());}
            s=decode(Some(&std::fs::read_to_string(path).map_err(|e|e.to_string())?))?;
        },
        _=>return Err("visual: new | clip | layer | set | key | ramp | blend | lfo | pulse | random | unmod | place | unplace | lock/unlock | load/save | on/off | export | inspect/params".into()),
    }
    Compiled::new(&s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visual_commands_are_atomic_and_lock_prevents_mutation() {
        let mut s = Score::default();
        for cmd in [
            "clip a 192",
            "layer a b rings",
            "place a 0 384 repeat",
            "set a b hue=0.3; scale=1.2",
            "ramp a b rotation 0:191 0:360",
            "lfo a b x 0.25 0.5 0",
            "pulse a b brightness 16 3 1 0",
            "lock",
        ] {
            s = edit(&s, cmd).unwrap();
        }
        let before = s.clone();
        assert!(edit(&s, "set a b hue=0.8").is_err());
        assert!(edit(&s, "clear").is_err());
        assert_eq!(s, before);
        s = edit(&s, "unlock").unwrap();
        let before = s.clone();
        assert!(edit(&s, "set a b hue=0.8; scale=NaN").is_err());
        assert_eq!(s, before);
        assert!(edit(&s, "key a b opacity 192 0").is_err());
        assert_eq!(decode(Some(&encode(&s).unwrap())).unwrap(), s);
    }
}
