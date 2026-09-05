// Bloom chain, run at quarter resolution.
//
// bright-pass -> horizontal blur -> vertical blur, then composited by post.wgsl.
// Separable blur: two 9-tap passes instead of one 81-tap kernel.

struct Params {
    resolution: vec2<f32>,
    bloom: f32,
    scanline: f32,
    aberration: f32,
    vignette: f32,
    grain: f32,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    let x = f32((idx << 1u) & 2u);
    let y = f32(idx & 2u);
    var out: VsOut;
    out.uv = vec2<f32>(x, y);
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

// Keep only what glows. Phosphor is bright and thin, so the knee sits high --
// a low threshold would bloom the body text into mush. With the current
// palette, RULE and EDGE land around 0.21 and 0.34, at or under the knee, so
// chassis hairlines stay crisp. CHASSIS reaches about 0.55 and lifts only
// slightly, while text and state colours near the top of the knee glow fully.
@fragment
fn fs_bright(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(src, smp, in.uv).rgb;
    let luma = max(max(c.r, c.g), c.b);
    return vec4<f32>(c * smoothstep(0.30, 0.85, luma), 1.0);
}

const W0: f32 = 0.227027;
const W1: f32 = 0.194595;
const W2: f32 = 0.121622;
const W3: f32 = 0.054054;
const W4: f32 = 0.016216;

fn blur(uv: vec2<f32>, dir: vec2<f32>) -> vec3<f32> {
    let texel = dir / vec2<f32>(textureDimensions(src));
    var c = textureSample(src, smp, uv).rgb * W0;
    c += textureSample(src, smp, uv + texel * 1.0).rgb * W1;
    c += textureSample(src, smp, uv - texel * 1.0).rgb * W1;
    c += textureSample(src, smp, uv + texel * 2.0).rgb * W2;
    c += textureSample(src, smp, uv - texel * 2.0).rgb * W2;
    c += textureSample(src, smp, uv + texel * 3.0).rgb * W3;
    c += textureSample(src, smp, uv - texel * 3.0).rgb * W3;
    c += textureSample(src, smp, uv + texel * 4.0).rgb * W4;
    c += textureSample(src, smp, uv - texel * 4.0).rgb * W4;
    return c;
}

@fragment
fn fs_blur_h(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(blur(in.uv, vec2<f32>(2.0, 0.0)), 1.0);
}

@fragment
fn fs_blur_v(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(blur(in.uv, vec2<f32>(0.0, 2.0)), 1.0);
}
