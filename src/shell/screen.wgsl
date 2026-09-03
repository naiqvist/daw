@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local_uv: vec2<f32>,
    @location(2) @interpolate(flat) rect: vec4<f32>,
    @location(3) @interpolate(flat) state: vec4<f32>,
};

@vertex
fn vs(
    @builtin(vertex_index) index: u32,
    @location(0) rect: vec4<f32>,
    @location(1) state: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    let local_uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    let uv = rect.xy + local_uv * (rect.zw - rect.xy);
    out.uv = uv;
    out.local_uv = local_uv;
    out.rect = rect;
    out.state = state;
    out.clip = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    return out;
}

fn emission(uv: vec2<f32>, rect: vec4<f32>) -> vec3<f32> {
    if uv.x <= rect.x || uv.y <= rect.y || uv.x >= rect.z || uv.y >= rect.w {
        return vec3<f32>(0.0);
    }
    let source = textureSample(frame, frame_sampler, uv).rgb;
    let light = dot(source, vec3<f32>(0.2126, 0.7152, 0.0722));
    let gate = max((light - 0.075) * 1.35, 0.0);
    return source * gate;
}

fn luma(colour: vec3<f32>) -> f32 {
    return dot(colour, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// Integer noise keeps the grain pixel-sharp. There is no blurred noise
// texture stretched over the panel: every value belongs to one screen pixel
// and one compositor frame.
fn hash_u32(value: u32) -> u32 {
    var x = value;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    return x ^ (x >> 16u);
}

fn analog_noise(pixel: vec2<u32>, tick: u32) -> f32 {
    let seed = pixel.x * 0x9e3779b9u ^ pixel.y * 0x85ebca6bu ^ tick * 0xc2b2ae35u;
    return f32(hash_u32(seed) & 0x00ffffffu) / 16777215.0;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    // The three vertices form the usual oversized full-frame triangle.
    // Reject only its overhang; the material mask below decides which frame
    // pixels are screens.
    if in.uv.x < in.rect.x || in.uv.y < in.rect.y || in.uv.x > in.rect.z || in.uv.y > in.rect.w {
        discard;
    }
    let dimensions = vec2<f32>(textureDimensions(frame));
    let pixel = vec2<f32>(1.0) / dimensions;
    let screen_pixel = floor(in.uv * dimensions);
    let pixel_address = vec2<u32>(screen_pixel);
    let tick = u32(in.state.z);

    // A real analogue line occasionally loses horizontal lock for one pixel.
    // It is a hard, short displacement rather than a soft deformation of the
    // whole aperture. A second seed keeps left and right equally likely.
    let line_seed = analog_noise(vec2<u32>(0u, pixel_address.y), tick / 2u);
    let direction_seed = analog_noise(vec2<u32>(pixel_address.y, 19u), tick / 3u);
    let line_direction = select(-1.0, 1.0, direction_seed > 0.5);
    let line_kick = select(0.0, line_direction, line_seed > 0.995);
    let safe_min = in.rect.xy + pixel * 0.5;
    let safe_max = in.rect.zw - pixel * 0.5;
    let signal_uv = clamp(in.uv + vec2<f32>(line_kick * pixel.x, 0.0), safe_min, safe_max);
    let authored = textureSample(frame, frame_sampler, in.uv);
    let base = textureSample(frame, frame_sampler, signal_uv);
    let red_sample = textureSample(
        frame,
        frame_sampler,
        clamp(signal_uv + vec2<f32>(pixel.x, 0.0), safe_min, safe_max),
    ).r;
    let blue_sample = textureSample(
        frame,
        frame_sampler,
        clamp(signal_uv - vec2<f32>(pixel.x, 0.0), safe_min, safe_max),
    ).b;

    // Discrete phosphor bleed: long along the beam, short between scanlines.
    // These are actual neighbouring glyph/curve pixels, not a radial overlay.
    var beam = vec3<f32>(0.0);
    beam += emission(signal_uv + vec2<f32>( 1.0,  0.0) * pixel, in.rect) * 0.34;
    beam += emission(signal_uv + vec2<f32>(-1.0,  0.0) * pixel, in.rect) * 0.34;
    beam += emission(signal_uv + vec2<f32>( 2.0,  0.0) * pixel, in.rect) * 0.22;
    beam += emission(signal_uv + vec2<f32>(-2.0,  0.0) * pixel, in.rect) * 0.22;
    beam += emission(signal_uv + vec2<f32>( 4.0,  0.0) * pixel, in.rect) * 0.12;
    beam += emission(signal_uv + vec2<f32>(-4.0,  0.0) * pixel, in.rect) * 0.12;
    beam += emission(signal_uv + vec2<f32>( 0.0,  1.0) * pixel, in.rect) * 0.16;
    beam += emission(signal_uv + vec2<f32>( 0.0, -1.0) * pixel, in.rect) * 0.16;
    beam += emission(signal_uv + vec2<f32>( 0.0,  2.0) * pixel, in.rect) * 0.07;
    beam += emission(signal_uv + vec2<f32>( 0.0, -2.0) * pixel, in.rect) * 0.07;

    let scanline = select(0.76, 1.0, (u32(screen_pixel.y) & 1u) == 0u);
    let triad = u32(screen_pixel.x) % 3u;
    let mask = vec3<f32>(
        select(0.86, 1.0, triad == 0u),
        select(0.86, 1.0, triad == 1u),
        select(0.86, 1.0, triad == 2u),
    );

    // Every bright element emits a little. Real signal/transport activity
    // opens it further, with a small decay across the current beat.
    let activity = clamp(in.state.x, 0.0, 1.0);
    let beat_decay = 1.0 - clamp(in.state.y, 0.0, 1.0);
    let bloom = 0.24 + activity * (0.18 + beat_decay * 0.08);
    let luminance = luma(base.rgb);

    // Black is the screen material. Looking four physical pixels around the
    // current one carries bright glyphs, cursors, traces and needles with the
    // dark field they sit in, while a genuinely light panel remains exact.
    // In this linear texture, 0.05 is roughly sRGB 63 and 0.13 roughly 101.
    let context_luminance = min(
        luma(authored.rgb),
        min(
            min(
                luma(textureSample(frame, frame_sampler, clamp(in.uv + vec2<f32>( 4.0,  0.0) * pixel, safe_min, safe_max)).rgb),
                luma(textureSample(frame, frame_sampler, clamp(in.uv + vec2<f32>(-4.0,  0.0) * pixel, safe_min, safe_max)).rgb),
            ),
            min(
                luma(textureSample(frame, frame_sampler, clamp(in.uv + vec2<f32>( 0.0,  4.0) * pixel, safe_min, safe_max)).rgb),
                luma(textureSample(frame, frame_sampler, clamp(in.uv + vec2<f32>( 0.0, -4.0) * pixel, safe_min, safe_max)).rgb),
            ),
        ),
    );
    let screen_gate = clamp((0.13 - context_luminance) / 0.08, 0.0, 1.0);

    // One physical pixel of RGB misregistration, admitted only by bright
    // phosphor. Mixing rather than replacing keeps it a fringe, not a split
    // image, and the green channel remains the stable centre line.
    let aberration_gate = clamp((luminance - 0.10) * 0.90, 0.0, 0.18);
    let separated = vec3<f32>(red_sample, base.g, blue_sample);
    let phosphor = mix(base.rgb, separated, aberration_gate);

    // Fine luma grain lives mostly in the phosphor, with just enough in the
    // black floor to make the glass feel electrically awake. A rare bright
    // row is the screen's horizontal interference, never a panel-wide wash.
    let grain = analog_noise(pixel_address, tick) - 0.5;
    let grain_level = 0.005 + min(luminance * 0.018, 0.018) + activity * 0.003;
    let interference_seed = analog_noise(vec2<u32>(31u, pixel_address.y), tick);
    let interference = select(0.0, 0.020 + activity * 0.010, interference_seed > 0.996);
    let noise_tint = vec3<f32>(0.72, 1.0, 0.91);

    let crt = max(
        phosphor * mask * scanline
            + beam * bloom
            + noise_tint * (grain * grain_level + interference),
        vec3<f32>(0.0),
    );
    return vec4<f32>(mix(authored.rgb, crt, screen_gate), authored.a);
}
