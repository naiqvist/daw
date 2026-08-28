// The pass that runs after everything.
//
// PORTED from ~/.config/hypr/shaders/crt-screen.frag, the screen shader
// this machine runs Hyprland with, plus the grade from cyberpunk.glsl
// beside it. Their tuning, their structure, their constants — the point
// of the port is that the app wears the same identity on a machine
// whose compositor is not doing it.
//
// # What changed in the port, and why
//
// 1. GAMMA SPACE, explicitly. Hyprland hands its shader a plain
//    `sampler2D` over an 8-bit surface, so its arithmetic is already on
//    ENCODED values. Our offscreen target is `Bgra8UnormSrgb`, so
//    `textureSample` decodes to LINEAR first. Running the ported
//    arithmetic there would be a different shader wearing the same
//    numbers: measured on this interface's own grounds, a multiplicative
//    modulation in linear space came to under one code value out of 255.
//    So we decode to gamma, do their arithmetic, and re-encode.
//
// 2. NO FRAME. `roundedBoxSDF` draws a bezel around the whole output;
//    inside one window it would be a border around a border.
//
// 3. CURVE DEFAULTS TO ZERO. Their note says it honestly — barrel
//    distortion "moves pixels away from where the pointer actually is".
//    On a whole desktop that is a few px you learn to live with; on a
//    device card with drag handles it is a control that is not where it
//    looks. It is here, it works, and it is off.
//
// 4. DITHER, which theirs does not have. Everything below produces
//    smooth ramps and a smooth ramp quantised to 8 bits is a stack of
//    visible bands.
//
// # Nothing here moves
//
// Their header states the reason and it applies to us for a different
// one: a `time` uniform in a Hyprland screen shader disables damage
// tracking compositor-wide, and in this app it would pin a full-rate
// repaint forever on a machine whose spare cycles belong to the audio
// thread. The grain is hashed from the pixel's own coordinates, so it
// sits in the glass rather than crawling over the content.

struct Uniforms {
    // The target's size in PHYSICAL pixels. Everything below is a pixel
    // grid effect, so it has to know the real grid rather than a scaled
    // one.
    resolution: vec2<f32>,
    // The master dial, exactly as theirs: 0 is a clean screen, 1 is the
    // tube as tuned below.
    intensity: f32,
    curve: f32,
    scanline: f32,
    grille: f32,
    bloom: f32,
    chroma: f32,
    vignette: f32,
    grain: f32,
    neon: f32,
    dither: f32,
    // Non-zero puts this shader in test-pattern mode; see the bottom of
    // `fs`.
    test: f32,
    // To sixty-four bytes. SCALARS, not a `vec3<f32>`: a vec3 aligns to
    // 16 in the uniform address space and would push this block to 80
    // while the Rust side stayed at 64. That is not a compile error in
    // either language — it is a validation error at the first draw.
    // `the_shader_and_the_struct_agree_on_the_layout` reads this file
    // and checks it, because review has now missed it twice.
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;
@group(0) @binding(2) var<uniform> u: Uniforms;

const TAU: f32 = 6.28318530718;

// --- their constants, carried over unchanged -------------------------
// Mirrored in `post.rs`'s test module, which is the only Rust that reads
// them. Change one, change both.

// Tube curvature.
const CURVE: f32 = 0.045;
// Aperture grille: one R/G/B stripe per screen pixel column, a triad
// every 3px. At 1920 wide that is 640 triads — about right for a 20"
// tube. `MASK_DEPTH` is how dark the two off-phosphors go.
const MASK_PITCH: f32 = 3.0;
// Theirs is 0.22. Raised, because a mask that deep costs most of the
// picture's light and this app's whole surface ladder — bg, card, well,
// sub-well — lives between 13 and 42 out of 255. Measured through the
// ported chain, 0.22 put sunken and bg both at ZERO: four distinct
// surfaces arriving as one black rectangle.
const MASK_DEPTH: f32 = 0.45;
// Scanlines every 2px. At 1px the pattern beats against text hinting
// and just looks like noise.
const SCAN_PITCH: f32 = 2.0;
// Theirs is 0.30. Same reason, and one more: a label here is ten pixels
// tall, so a glyph's stroke is two or three rows. Taking 30 % from every
// other one of them is taking it from most of the letter.
const SCAN_DEPTH: f32 = 0.22;
// Beam bloom: bright areas bleed into their neighbours the way an
// overdriven electron gun does. This is what stops the mask and the
// scanlines reading as a flat dark filter.
const BLOOM: f32 = 0.35;
// Corner chroma split, in px at the very corners. Convergence error.
const CHROMA: f32 = 1.1;
const VIGNETTE: f32 = 0.30;
const GRAIN: f32 = 4.0; // /255, fixed-pattern
// Phosphors are warm and the mask eats light, so pull the whole picture
// back up and give it a slight amber cast.
// Theirs is 1/0.68, which assumes the mask and the lines took 32 % of
// the light. Through the actual tile they take about 47 %, so their own
// screen sits at roughly 60 % brightness — fine on a desktop, and not
// fine on an instrument you read numbers off. This is tuned against the
// mask and scan depths above so a panel comes back to where it started.
const GAIN: f32 = 1.0 / 0.53;
const TINT: vec3<f32> = vec3<f32>(1.00, 0.97, 0.91);

// --- from cyberpunk.glsl ---------------------------------------------
// Saturation and contrast only. Its neon tint is a COOL push
// (0.1, 0.2, 0.3) and the tube's is warm amber; run both and they
// cancel into grey. The tube's cast is what makes the picture read as a
// tube, so the grade keeps the half that does not fight it.
const SAT_BOOST: f32 = 0.50;
// Theirs is 0.30 about a pivot of 0.5, which is right for a desktop
// full of mid-tones and catastrophic here: every surface this app has
// sits BELOW 0.17, so a contrast boost pivoted at the middle pushes all
// of them toward black together. Measured, it took the ladder from 29
// code values of separation down to 7.
//
// So the pivot moves to where this interface's tones actually are —
// between the sunken well and the raised card — and the boost then does
// what it is supposed to do: pushes the surfaces APART instead of
// pushing them all down.
const CONTRAST: f32 = 0.18;
const CONTRAST_PIVOT: f32 = 0.14;

// --- RELIEF ----------------------------------------------------------
//
// Where there is DETAIL, the tube backs off.
//
// This is the readability fix, and it is the same argument the rest of
// this app is built on: a mark earns its place by having a job. The
// mask, the lines and the convergence error exist to make a flat
// SURFACE feel like a tube. None of them has any business on a glyph —
// there they are three separate attacks on the one thing the interface
// exists to deliver, and small text is most of what is on screen.
//
// So local contrast decides. On a flat panel it is zero and the tube
// runs at full; across the edge of a letter or a hairline it is large
// and the tube fades out, leaving the pixel as egui drew it. The
// texture stays exactly where it belongs and leaves the reading alone.
const RELIEF_LO: f32 = 0.05;
const RELIEF_HI: f32 = 0.28;
// How far it backs off at most. Not all the way: a letter with NO tube
// on it at all sits on the surface rather than in it, and the edge of
// the relief becomes visible as a halo.
const RELIEF: f32 = 0.88;

// --- ours ------------------------------------------------------------
// ±half a code value: enough to break a band into a pattern the eye
// integrates away, and by construction never enough to be seen.
const DITHER_CEILING: f32 = 0.5 / 255.0;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) index: u32) -> VsOut {
    var out: VsOut;
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    out.uv = uv;
    out.clip = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    return out;
}

// The sRGB transfer function, both ways. See point 1 of the header.
fn to_gamma(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// Sample the frame in ENCODED units, which is the space everything
// below works in.
fn tap(uv: vec2<f32>) -> vec3<f32> {
    return to_gamma(textureSample(frame, frame_sampler, uv).rgb);
}

// Theirs, unchanged. Measured across a 256x256 grid in f32 — which is
// what a GPU runs — it gives a mean of -0.002, a variance of 0.0833
// (uniform on ±0.5 is exactly 1/12) and no adjacent-pixel correlation.
//
// Worth stating because the obvious alternative is not: the
// `fract(sin(dot(p, k)) * 43758.5453)` hash every shader on the
// internet uses measures a mean of -0.500 and a correlation of +0.27 in
// f32. That is not grain, it is a constant darkening with streaks in it.
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// The 4x4 ordered dither matrix, normalised to ±0.5. Ordered rather
// than random because a pattern this small reads as texture while
// random at the same amplitude reads as dirt.
fn bayer(at: vec2<f32>) -> f32 {
    let x = u32(at.x) & 3u;
    let y = u32(at.y) & 3u;
    var m = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0,
    );
    return m[y * 4u + x] / 16.0 - 0.46875;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let size = u.resolution;
    let pos = floor(in.clip.xy);
    let px = vec2<f32>(1.0, 1.0) / size;
    let alpha = textureSample(frame, frame_sampler, in.uv).a;

    // --- Tube geometry ----------------------------------------------
    let ctr = in.uv - vec2<f32>(0.5);
    let r2 = dot(ctr, ctr);
    // Clamped: with no frame to hide the overscan, an edge pixel that
    // sampled outside would smear.
    let uv = clamp(
        in.uv + ctr * r2 * (CURVE * u.curve * u.intensity),
        vec2<f32>(0.0),
        vec2<f32>(1.0),
    );

    // --- The relief measure, first -----------------------------------
    //
    // Four taps a pixel out. The largest luminance step to a neighbour
    // says whether this pixel is in the middle of a surface or on the
    // edge of a letter, and everything that models the TUBE is scaled
    // by the answer.
    //
    // It is computed BEFORE the convergence split, and from
    // un-split taps, for two reasons: the split itself must ride the
    // relief — colour fringing on the edge of a glyph is the most
    // legible-looking damage this whole pass can do — and a measure
    // taken from already-aberrated samples would be measuring its own
    // output.
    let centre = tap(uv);
    let n0 = tap(uv + vec2<f32>(px.x, 0.0));
    let n1 = tap(uv + vec2<f32>(-px.x, 0.0));
    let n2 = tap(uv + vec2<f32>(0.0, px.y));
    let n3 = tap(uv + vec2<f32>(0.0, -px.y));

    let here = luma(centre);
    let step_to = max(
        max(abs(luma(n0) - here), abs(luma(n1) - here)),
        max(abs(luma(n2) - here), abs(luma(n3) - here)),
    );
    let relief = smoothstep(RELIEF_LO, RELIEF_HI, step_to) * RELIEF;
    // The vignette, the grain and the dither are properties of the
    // GLASS rather than of the beam, so they do not ride this.
    let tube = u.intensity * (1.0 - relief);

    // --- Convergence -------------------------------------------------
    let off = normalize(ctr + vec2<f32>(1e-6)) * (r2 * CHROMA * 4.0 * u.chroma * tube) / size;
    var colour = select(
        vec3<f32>(tap(uv + off).r, centre.g, tap(uv - off).b),
        centre,
        off.x == 0.0 && off.y == 0.0,
    );

    // --- Beam bloom --------------------------------------------------
    //
    // The same four taps, SQUARED so only highlights spread: at a
    // ground of 0.08 the square is 0.006 and contributes nothing, while
    // a readout at 0.9 keeps almost all of itself. That is the whole
    // bright-pass, and it costs one multiply.
    let glow = (n0 + n1 + n2 + n3) * 0.25;
    colour = colour + glow * glow * (BLOOM * u.bloom * u.intensity);

    // --- Aperture grille ---------------------------------------------
    //
    // Column index mod 3 picks which phosphor is lit. `floor` on the
    // real pixel position, so the stripes stay locked to the panel grid
    // and never moire.
    let stripe = i32(pos.x - MASK_PITCH * floor(pos.x / MASK_PITCH));
    var mask = vec3<f32>(MASK_DEPTH, MASK_DEPTH, MASK_DEPTH);
    if (stripe == 0) {
        mask.r = 1.0;
    } else if (stripe == 1) {
        mask.g = 1.0;
    } else {
        mask.b = 1.0;
    }
    colour = colour * mix(vec3<f32>(1.0), mask, u.grille * tube);

    // --- Scanlines ---------------------------------------------------
    //
    // Raised cosine rather than a hard stripe: a beam has a soft
    // vertical profile, and the soft version does not alias when the
    // pitch is not integral.
    let line = 0.5 + 0.5 * cos(TAU * pos.y / SCAN_PITCH);
    colour = colour * (1.0 - SCAN_DEPTH * u.scanline * tube * line);

    // --- Tube response -----------------------------------------------
    //
    // The mask and the scanlines are pure attenuation — between them
    // they throw away most of the light — so the gain puts it back and
    // the tint says what kind of glass it went through. This is the
    // step that lets the mask be as deep as it is; without it the whole
    // picture would simply be dark.
    //
    // THE ONE PLACE THIS DEVIATES FROM THE PORT. Theirs multiplies
    // outright, which is correct on average and overshoots at the peak:
    // a white glyph on its own lit phosphor stripe, in the gap between
    // two beam passes, takes the full 1.47 with nothing subtracted
    // first, lands at 1.315 and clips. On a desktop that is a highlight
    // blooming. Here it is one column in three of every piece of small
    // white text going flat, and this interface is mostly small white
    // text.
    //
    // So the gain is applied as `x·g / (1 + (g-1)·x)` instead. Same
    // slope at the bottom — the darks, which is where the mask actually
    // took the light — asymptotically unity at the top, monotonic
    // everywhere, and exactly the identity at g = 1 so the bypass
    // survives. It cannot clip by construction rather than by tuning.
    let lit = u.grille * u.scanline * tube;
    let g = mix(1.0, GAIN, lit);
    colour = colour * g / (vec3<f32>(1.0) + (g - 1.0) * colour);
    colour = colour * mix(vec3<f32>(1.0), TINT, lit);

    // --- Neon grade, from cyberpunk.glsl ------------------------------
    let grade = u.neon * tube;
    let grey = vec3<f32>(dot(colour, vec3<f32>(0.299, 0.587, 0.114)));
    colour = mix(grey, colour, 1.0 + SAT_BOOST * grade);
    // The expansion RELAXES as it approaches white. Its job is to push
    // the surfaces apart — they all live below 0.17 — and a value
    // already at the top has nothing to be pushed toward: expanding it
    // only sends it through the ceiling, which is a white readout going
    // flat. Measured at the ported settings, a white block clipped
    // outright.
    let headroom = vec3<f32>(1.0) - smoothstep(vec3<f32>(0.75), vec3<f32>(1.0), colour);
    colour = (colour - vec3<f32>(CONTRAST_PIVOT)) * (vec3<f32>(1.0) + CONTRAST * grade * headroom)
        + vec3<f32>(CONTRAST_PIVOT);

    // --- Vignette, grain, dither -------------------------------------
    colour = colour * (1.0 - r2 * VIGNETTE * 2.0 * u.vignette * u.intensity);
    colour = colour + (hash12(pos) - 0.5) * (GRAIN / 255.0) * u.grain * u.intensity;
    colour = colour + bayer(pos) * DITHER_CEILING * u.dither;

    let out = clamp(colour, vec3<f32>(0.0), vec3<f32>(1.0));

    // --- TEST PATTERN -------------------------------------------------
    //
    // A diagnostic, and deliberately hideous. If THIS is not on screen
    // the pass is not reaching the glass, and nothing about the tuning
    // above is the problem.
    if (u.test > 0.5) {
        let bar = select(0.35, 1.0, line > 0.5);
        var tint = vec3<f32>(0.15, 0.15, 0.15);
        if (stripe == 0) {
            tint.r = 1.0;
        } else if (stripe == 1) {
            tint.g = 1.0;
        } else {
            tint.b = 1.0;
        }
        let wash = mix(out, tint, 0.6) * bar;
        return vec4<f32>(to_linear(clamp(wash, vec3<f32>(0.0), vec3<f32>(1.0))), alpha);
    }

    return vec4<f32>(to_linear(out), alpha);
}
