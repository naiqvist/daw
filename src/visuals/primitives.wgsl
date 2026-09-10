// Reusable stateless visual kernels. Same pipeline in preview and export.
struct Layer { transform: vec4<f32>, colour: vec4<f32>, shape: vec4<f32>, kind: vec4<u32> }
struct Frame { background: vec4<f32>, info: vec4<f32>, layers: array<Layer,16> }
@group(0) @binding(0) var<uniform> frame: Frame;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) id: u32) -> Vertex {
    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    return Vertex(vec4<f32>(uv * 2.0 - 1.0,0.0,1.0),vec2<f32>(uv.x,1.0-uv.y));
}
fn hash(p: vec2<f32>, seed: f32) -> f32 {
    return fract(sin(dot(p,vec2<f32>(127.1,311.7))+seed)*43758.5453);
}
fn noise(p: vec2<f32>, seed: f32) -> f32 {
    let a = floor(p); let f = fract(p); let u = f*f*(3.0-2.0*f);
    return mix(mix(hash(a,seed),hash(a+vec2<f32>(1.0,0.0),seed),u.x),
        mix(hash(a+vec2<f32>(0.0,1.0),seed),hash(a+vec2<f32>(1.0,1.0),seed),u.x),u.y);
}
fn palette(h: f32, saturation: f32) -> vec3<f32> {
    let p = abs(fract(vec3<f32>(h)+vec3<f32>(0.0,0.6666667,0.3333333))*6.0-3.0);
    return mix(vec3<f32>(1.0),clamp(p-1.0,vec3<f32>(0.0),vec3<f32>(1.0)),saturation);
}
fn transform(uv: vec2<f32>, l: Layer) -> vec2<f32> {
    let p = (uv*2.0-1.0)*vec2<f32>(frame.info.x,1.0)-l.transform.xy;
    let a = l.transform.w; let c = cos(a); let s = sin(a);
    return vec2<f32>(c*p.x-s*p.y,s*p.x+c*p.y)/l.transform.z;
}
fn warp(p: vec2<f32>, amount: f32, phase: f32) -> vec2<f32> {
    return p + amount*0.25*vec2<f32>(sin(p.y*3.0+phase),sin(p.x*3.7-phase*0.71));
}
fn primitive(p: vec2<f32>, l: Layer) -> vec2<f32> {
    let f = l.shape.x; let soft = max(l.shape.z,0.001); let phase = l.shape.w;
    let r = length(p); var v = 0.0; var tone = 0.0;
    switch l.kind.x {
        case 0u: { v = 1.0-smoothstep(0.43,0.43+soft,r); tone = r; }
        case 1u: {
            let d = abs(sin(r*f*3.14159265-phase));
            v = exp(-d*d/max(soft*soft,0.001))*exp(-r*r*0.45); tone = r*0.15;
        }
        case 2u: {
            let path = sin(p.x*f+phase)*0.22 + sin(p.x*f*0.47-phase*0.5)*0.16;
            let d = abs(p.y-path); v = exp(-d*d/max(soft*soft,0.0001));
            v *= exp(-p.x*p.x*0.25); tone = p.x*0.08;
        }
        case 3u: {
            let a = sin(p.x*f+phase)+sin(p.y*f*0.7-phase*0.61)+sin((p.x+p.y)*f*0.5+phase*0.4);
            tone = a*0.12; v = 0.28+0.35*(a/3.0+1.0);
        }
        default: { v = noise(p*f+vec2<f32>(phase*0.1),f32(l.kind.z%65536u)); tone = v*0.2; }
    }
    return vec2<f32>(v,tone);
}
fn blend(dst: vec3<f32>, src: vec3<f32>, alpha: f32, mode: u32) -> vec3<f32> {
    switch mode {
        case 1u: { return dst+src*alpha; }
        case 2u: { return mix(dst,dst*src,alpha); }
        case 3u: { return 1.0-(1.0-clamp(dst,vec3<f32>(0.0),vec3<f32>(1.0)))*(1.0-clamp(src*alpha,vec3<f32>(0.0),vec3<f32>(1.0))); }
        default: { return mix(dst,src,alpha); }
    }
}
@fragment fn fs(v: Vertex) -> @location(0) vec4<f32> {
    var colour = frame.background.rgb;
    for (var i = 0u; i < u32(frame.info.y); i += 1u) {
        let l = frame.layers[i];
        let p = warp(transform(v.uv,l),l.shape.y,l.shape.w);
        let field = primitive(p,l);
        let source = palette(l.colour.x+field.y,l.colour.y)*l.colour.z;
        colour = blend(colour,source,clamp(field.x*l.colour.w,0.0,1.0),l.kind.y);
    }
    // Fixed exposure/soft shoulder; no hidden time, flicker or feedback history.
    colour = max(colour,vec3<f32>(0.0));
    colour = colour/(1.0+colour);
    if frame.info.z > 0.5 {
        colour = select(1.055*pow(colour,vec3<f32>(1.0/2.4))-0.055,colour*12.92,colour<=vec3<f32>(0.0031308));
    }
    return vec4<f32>(colour,1.0);
}
