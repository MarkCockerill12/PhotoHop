// PhotoHop compositor. Working space: scene-linear, premultiplied RGBA in rgba16float.
// Blend formulas follow GIMP's non-legacy layer modes with "union" compositing
// (AiContext research/GIMP_COMPOSITING.md §1.3, §1.5).

struct LayerParams {
    // Inverse transform rows: canvas pixel -> source pixel.  src = m0.xy*x + m1.xy*y + m2.xy
    inv0: vec4<f32>,
    inv1: vec4<f32>,
    inv2: vec4<f32>,
    src_size: vec2<f32>,
    opacity: f32,
    blend: u32,
    perceptual: u32,
    has_mask: u32,
    mask_invert: u32,
    has_grade: u32,
    // Grade (see ph-core grade.rs for the order of operations).
    g_basic: vec4<f32>,   // exposure, temperature, tint, enabled
    g_lift: vec4<f32>,
    g_gamma: vec4<f32>,
    g_gain: vec4<f32>,
    g_offset: vec4<f32>,
    g_tone: vec4<f32>,    // contrast, pivot, saturation, match amount
    g_mscale: vec4<f32>,
    g_moffset: vec4<f32>,
};

@group(0) @binding(0) var backdrop: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var src_tex: texture_2d<f32>;
@group(0) @binding(3) var src_sampler: sampler;
@group(0) @binding(4) var<uniform> P: LayerParams;
@group(0) @binding(5) var mask_tex: texture_2d<f32>;

fn to_perceptual(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}

fn from_perceptual(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn safe_div(a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    let small = abs(b) <= vec3<f32>(1e-6);
    return clamp(select(a / b, vec3<f32>(0.0), small), vec3<f32>(-1e6), vec3<f32>(1e6));
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// `b` = backdrop, `l` = layer, both straight-alpha colour in the mode's blend space.
fn blend(mode: u32, b: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let half = vec3<f32>(0.5);
    let one = vec3<f32>(1.0);
    switch mode {
        case 1u: { return b * l; }                                     // Multiply
        case 2u: { return one - (one - b) * (one - l); }               // Screen
        case 3u: {                                                     // Overlay
            return select(one - 2.0 * (one - l) * (one - b), 2.0 * b * l, b < half);
        }
        case 4u: { return (one - b) * b * l + b * (one - (one - b) * (one - l)); } // Soft light
        case 5u: {                                                     // Hard light
            let hi = min(one - (one - b) * (one - (l - half) * 2.0), one);
            let lo = min(b * l * 2.0, one);
            return select(lo, hi, l > half);
        }
        case 6u: { return b + l; }                                     // Addition
        case 7u: { return b - l; }                                     // Subtract
        case 8u: { return abs(b - l); }                                // Difference
        case 9u: { return min(b, l); }                                 // Darken only
        case 10u: { return max(b, l); }                                // Lighten only
        case 11u: { return safe_div(b, one - l); }                     // Dodge
        case 12u: { return one - safe_div(one - b, l); }               // Burn
        case 13u: { return b + 2.0 * l - one; }                        // Linear light
        case 14u: { return b - l + half; }                             // Grain extract
        case 15u: { return b + l - half; }                             // Grain merge
        case 16u: { return safe_div(b, l); }                           // Divide
        case 17u: { return half - 2.0 * (b - half) * (l - half); }     // Exclusion
        case 18u: {                                                    // Luminance
            let lb = luma(b);
            return select(b * (luma(l) / lb), vec3<f32>(luma(l)), lb <= 1e-6);
        }
        default: { return l; }                                         // Normal
    }
}

fn grade(lin: vec3<f32>) -> vec3<f32> {
    // Linear stage: exposure and white balance (gains normalised to keep luminance).
    var c = lin * exp2(P.g_basic.x);
    let wb = vec3<f32>(exp(0.25 * P.g_basic.y), exp(-0.25 * P.g_basic.z), exp(-0.25 * P.g_basic.y));
    c = c * (wb / luma(wb));
    // Perceptual stage.
    var p = to_perceptual(c);
    p = mix(p, p * P.g_mscale.rgb + P.g_moffset.rgb, P.g_tone.w);
    let lift = P.g_lift.rgb + P.g_lift.w;
    let gain = P.g_gain.rgb * P.g_gain.w;
    p = p * gain + lift * (vec3<f32>(1.0) - p);
    let gexp = exp(-(P.g_gamma.rgb + P.g_gamma.w));
    p = sign(p) * pow(abs(p), gexp);
    p = (p - P.g_tone.y) * P.g_tone.x + P.g_tone.y;
    let y = luma(p);
    p = y + (p - y) * P.g_tone.z;
    p = p + P.g_offset.rgb + P.g_offset.w;
    return from_perceptual(max(p, vec3<f32>(0.0)));
}

fn sample_layer(p: vec2<f32>) -> vec4<f32> {
    let s = P.inv0.xy * p.x + P.inv1.xy * p.y + P.inv2.xy;
    if (s.x < 0.0 || s.y < 0.0 || s.x > P.src_size.x || s.y > P.src_size.y) {
        return vec4<f32>(0.0);
    }
    let uv = s / P.src_size;
    var c = textureSampleLevel(src_tex, src_sampler, uv, 0.0);
    if (P.has_mask != 0u) {
        var m = textureSampleLevel(mask_tex, src_sampler, uv, 0.0).r;
        if (P.mask_invert != 0u) { m = 1.0 - m; }
        c = c * clamp(m, 0.0, 1.0);
    }
    return c;
}

@compute @workgroup_size(8, 8)
fn composite(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(dst);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    let px = vec2<i32>(gid.xy);
    let back = textureLoad(backdrop, px, 0);
    let lay = sample_layer(vec2<f32>(gid.xy) + vec2<f32>(0.5));

    let a_in = back.a;
    let a_l = clamp(lay.a * P.opacity, 0.0, 1.0);
    if (a_l <= 0.0) { textureStore(dst, px, back); return; }

    var l = lay.rgb / max(lay.a, 1e-6);
    if (P.has_grade != 0u) { l = grade(l); }
    if (a_in <= 0.0) { textureStore(dst, px, vec4<f32>(l * a_l, a_l)); return; }
    let b = back.rgb / a_in;

    var c: vec3<f32>;
    if (P.perceptual != 0u) {
        c = from_perceptual(blend(P.blend, to_perceptual(b), to_perceptual(l)));
    } else {
        c = blend(P.blend, b, l);
    }
    // GIMP union: out = in + (a_l/a_out) * (a_in*(c - layer) + layer - in)
    let a_out = a_l + (1.0 - a_l) * a_in;
    let rgb = b + (a_l / a_out) * (a_in * (c - l) + l - b);
    textureStore(dst, px, vec4<f32>(rgb * a_out, a_out));
}

// ---- background fill ----
struct ClearParams { color: vec4<f32> };
@group(0) @binding(0) var clear_dst: texture_storage_2d<rgba16float, write>;
@group(0) @binding(1) var<uniform> C: ClearParams;

@compute @workgroup_size(8, 8)
fn clear(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(clear_dst);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    textureStore(clear_dst, vec2<i32>(gid.xy), vec4<f32>(C.color.rgb * C.color.a, C.color.a));
}
