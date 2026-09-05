// The post-process pass: the only thing that writes to the swapchain.
// The console's glass, shared with the cockpit (~/Work/tachikoma).
//
// Everything the stage draws lands in
// one offscreen texture and passes through here, so the treatment is uniform.
//
// Restraint is deliberate. This is a terminal you read code in: aberration
// stays under a pixel and the scanline only takes ~10%, so stems stay solid.
// The glow does the work. No curvature: it warps a rectilinear grid of text,
// which is exactly the content it looks worst on.
//
// Nothing here varies with time. That is a design choice, not an omission:
// animated tearing and roll read as gimmicks, and driving them cost ~50% of a
// core in continuous repaint. The treatment is static, so an idle window
// costs nothing.

struct Params {
    resolution: vec2<f32>,
    bloom: f32,
    scanline: f32,
    aberration: f32,
    vignette: f32,
    grain: f32,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var scene: texture_2d<f32>;
@group(0) @binding(2) var bloom_tex: texture_2d<f32>;
@group(0) @binding(3) var smp: sampler;

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

fn hash(v: vec2<f32>) -> f32 {
    return fract(sin(dot(v, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;

    // Radial chromatic aberration -- zero at the centre, growing to the edges,
    // which is how real glass behaves.
    let from_centre = uv - vec2<f32>(0.5);
    let ca = from_centre * (p.aberration / p.resolution.x);
    var col = vec3<f32>(
        textureSample(scene, smp, uv + ca).r,
        textureSample(scene, smp, uv).g,
        textureSample(scene, smp, uv - ca).b,
    );

    col += textureSample(bloom_tex, smp, uv).rgb * p.bloom;

    // Scanlines on a 2-physical-pixel period. Cosine rather than a step so it
    // does not alias into moire when the window is an odd height.
    let line = 0.5 + 0.5 * cos(uv.y * p.resolution.y * 3.14159265);
    col *= 1.0 - p.scanline * line;

    let d = length(from_centre);
    // Glass loses long wavelengths first at the field edge, so the falloff
    // settles toward the cool ground instead of reading as a dark filter.
    let v = p.vignette * d * d;
    col *= vec3<f32>(1.0 - v * 1.15, 1.0 - v, 1.0 - v * 0.85);

    // Fixed per pixel, not per frame: static grain reads as tube texture,
    // animated grain reads as noise and forces a repaint every frame.
    col += (hash(uv * p.resolution) - 0.5) * p.grain;

    return vec4<f32>(col, 1.0);
}
