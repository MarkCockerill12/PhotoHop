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
    g_qual0: vec4<f32>,   // hue centre, hue half-width, softness, enabled
    g_qual1: vec4<f32>,   // sat lo, sat hi, lum lo, lum hi
    g_qual2: vec4<f32>,   // hue shift, saturation, gain, flags (1 invert, 2 show matte)
    has_curves: u32,
    _e0: u32,
    _e1: u32,
    _e2: u32,
};

@group(0) @binding(0) var backdrop: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var src_tex: texture_2d<f32>;
@group(0) @binding(3) var src_sampler: sampler;
@group(0) @binding(4) var<uniform> P: LayerParams;
@group(0) @binding(5) var mask_tex: texture_2d<f32>;
@group(0) @binding(6) var<uniform> curve_lut: array<vec4<f32>, 256>;

struct FxParams {
    wrap: vec4<f32>,      // light-wrap amount, wrap width (canvas px), blur (canvas px), sharpen
    grain: vec4<f32>,     // amount, size (canvas px), mono, frame seed
    relight0: vec4<f32>,  // light direction xyz, enabled
    relight1: vec4<f32>,  // light colour rgb, intensity
    relight2: vec4<f32>,  // ambient, rim, wrap (sss), amount
    relight3: vec4<f32>,  // rim colour rgb, has normals
    scale: vec4<f32>,     // canvas px -> source px, enabled, 0, 0
    haze: vec4<f32>,      // haze colour rgb, amount
    game: vec4<f32>,      // pixel block (canvas px), posterize levels, 0, 0
    dapple0: vec4<f32>,   // shadow offset per pixel of height (x, y), ground line y, ground tolerance (canvas px)
    dapple1: vec4<f32>,   // foot search step (canvas px)
    dapple2: vec4<f32>,
    dapple3: vec4<f32>,   // shade rgb (shadow/lit ratio), strength
    dapple4: vec4<f32>,   // softness (canvas px), enabled, 0, 0
};
@group(0) @binding(7) var<uniform> FX: FxParams;
@group(0) @binding(8) var normal_tex: texture_2d<f32>;
@group(0) @binding(9) var light_tex: texture_2d<f32>;

// How lit the scene is where this subject point's shadow would land (1 = sun, 0 = shadow).
// The point stands on the ground at the bottom of the silhouette in its own column (so a body
// with depth, or legs apart, gets the right foot), unless that bottom is well above the ground
// line (an outstretched arm), in which case it stands on the ground line.
fn borrowed_light(pc: vec2<f32>, dimsf: vec2<f32>) -> f32 {
    let ground = FX.dapple0.z;
    let tol = FX.dapple0.w;
    let limit = ground + tol;
    // Fixed step on a shared grid: every row of a column tests the same sample points.
    let stp = max(FX.dapple1.x, 1.0);
    var y = (floor(pc.y / stp) + 1.0) * stp;
    for (var i = 0u; i < 640u; i++) {
        if (y >= limit) { y = limit; break; }
        if (layer_alpha_at(vec2<f32>(pc.x, y)) < 0.5) { break; }
        y = y + stp;
    }
    var foot = y;
    if (foot < ground - tol) { foot = ground; }
    let h = max(foot - pc.y, 0.0);
    let g = vec2<f32>(pc.x, foot) + FX.dapple0.xy * h;
    var lit = textureSampleLevel(light_tex, src_sampler, g / dimsf, 0.0).r;
    if (FX.dapple4.x > 0.0) {
        for (var i = 0u; i < 16u; i++) {
            lit += textureSampleLevel(light_tex, src_sampler, (g + disk(i) * FX.dapple4.x) / dimsf, 0.0).r;
        }
        lit /= 17.0;
    }
    return clamp(lit, 0.0, 1.0);
}

const TAPS: u32 = 16u;

fn disk(i: u32) -> vec2<f32> {
    let r = sqrt((f32(i) + 0.5) / f32(TAPS));
    let a = f32(i) * 2.3999632;
    return vec2<f32>(cos(a), sin(a)) * r;
}

fn hash2(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(0.1031, 0.1030));
    q = q + dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}

fn to_source(p: vec2<f32>) -> vec2<f32> {
    return P.inv0.xy * p.x + P.inv1.xy * p.y + P.inv2.xy;
}

fn inside(s: vec2<f32>) -> bool {
    return s.x >= 0.0 && s.y >= 0.0 && s.x <= P.src_size.x && s.y <= P.src_size.y;
}

fn sample_src(s: vec2<f32>) -> vec4<f32> {
    if (!inside(s)) { return vec4<f32>(0.0); }
    return textureSampleLevel(src_tex, src_sampler, s / P.src_size, 0.0);
}

/// Layer alpha (with mask) at a canvas position, unblurred — used for light wrap.
fn layer_alpha_at(p: vec2<f32>) -> f32 {
    let s = to_source(p);
    if (!inside(s)) { return 0.0; }
    let uv = s / P.src_size;
    var a = textureSampleLevel(src_tex, src_sampler, uv, 0.0).a;
    if (P.has_mask != 0u) { a = a * mask_value(uv); }
    return a;
}

fn relight(l: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
    var n = vec3<f32>(0.0, 0.0, 1.0);
    if (FX.relight3.w > 0.5) {
        n = normalize(textureSampleLevel(normal_tex, src_sampler, uv, 0.0).rgb * 2.0 - 1.0);
    }
    let L = FX.relight0.xyz;
    let w = FX.relight2.z;
    let lambert = clamp((dot(n, L) + w) / (1.0 + w), 0.0, 1.0);
    let facing = clamp((L.z + w) / (1.0 + w), 0.05, 1.0);
    let amb = FX.relight2.x;
    let k = FX.relight1.w;
    // Shading ratio: a surface facing the camera keeps its brightness; others brighten/darken.
    let factor = (amb + k * lambert * FX.relight1.rgb) / (amb + k * facing);
    let rim = FX.relight2.y * pow(1.0 - clamp(n.z, 0.0, 1.0), 3.0) * FX.relight3.rgb;
    let relit = l * factor + rim * luma(l + 0.05);
    return mix(l, relit, FX.relight2.w);
}

const AX = vec3<f32>(1.0, -0.5, -0.5);
const BX = vec3<f32>(0.0, 0.8660254, -0.8660254);

fn lut_at(v: f32, ch: u32) -> f32 {
    let x = clamp(v, 0.0, 1.0) * 255.0;
    let i = u32(floor(x));
    let j = min(i + 1u, 255u);
    return mix(curve_lut[i][ch], curve_lut[j][ch], x - f32(i));
}

fn apply_curves(p: vec3<f32>) -> vec3<f32> {
    let c = vec3<f32>(lut_at(p.r, 0u), lut_at(p.g, 1u), lut_at(p.b, 2u));
    return vec3<f32>(lut_at(c.r, 3u), lut_at(c.g, 3u), lut_at(c.b, 3u));
}

// HSL-style key on perceptual input colour (hue 0..1, red = 0).
fn qualifier_key(p: vec3<f32>) -> f32 {
    let mx = max(p.r, max(p.g, p.b));
    let mn = min(p.r, min(p.g, p.b));
    let d = mx - mn;
    var h = 0.0;
    if (d > 1e-5) {
        if (mx == p.r) { h = (p.g - p.b) / d; }
        else if (mx == p.g) { h = 2.0 + (p.b - p.r) / d; }
        else { h = 4.0 + (p.r - p.g) / d; }
        h = fract(h / 6.0 + 1.0);
    }
    let l = (mx + mn) * 0.5;
    let s = select(d / (1.0 - abs(2.0 * l - 1.0) + 1e-6), 0.0, d < 1e-5);
    let soft = max(P.g_qual0.z, 1e-4);
    let hd = abs(fract(h - P.g_qual0.x + 0.5) - 0.5);
    let hk = 1.0 - smoothstep(P.g_qual0.y, P.g_qual0.y + soft, hd);
    let sk = smoothstep(P.g_qual1.x - soft, P.g_qual1.x, s) * (1.0 - smoothstep(P.g_qual1.y, P.g_qual1.y + soft, s));
    let y = luma(p);
    let lk = smoothstep(P.g_qual1.z - soft, P.g_qual1.z, y) * (1.0 - smoothstep(P.g_qual1.w, P.g_qual1.w + soft, y));
    var k = hk * sk * lk;
    if ((u32(P.g_qual2.w) & 1u) != 0u) { k = 1.0 - k; }
    return k;
}

fn qualifier_adjust(p: vec3<f32>) -> vec3<f32> {
    let y = luma(p);
    let c = p - y;
    let x0 = dot(c, AX) / 1.5;
    let y0 = dot(c, BX) / 1.5;
    let a = P.g_qual2.x * 6.2831853;
    let x1 = (x0 * cos(a) - y0 * sin(a)) * P.g_qual2.y;
    let y1 = (x0 * sin(a) + y0 * cos(a)) * P.g_qual2.y;
    return (y + x1 * AX + y1 * BX) * P.g_qual2.z;
}

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
    var key = 0.0;
    let qual_on = P.g_qual0.w > 0.5;
    if (qual_on) {
        key = qualifier_key(to_perceptual(lin));
        if ((u32(P.g_qual2.w) & 2u) != 0u) { return from_perceptual(vec3<f32>(key)); }
    }
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
    if (P.has_curves != 0u) { p = apply_curves(p); }
    p = p + P.g_offset.rgb + P.g_offset.w;
    if (qual_on) { p = mix(p, qualifier_adjust(p), key); }
    return from_perceptual(max(p, vec3<f32>(0.0)));
}

/// Mask coverage at a source uv. Flags: bit 0 = invert, bit 1 = mask decoded from a video (sRGB).
fn mask_value(uv: vec2<f32>) -> f32 {
    var m = textureSampleLevel(mask_tex, src_sampler, uv, 0.0).r;
    if ((P.mask_invert & 2u) != 0u) { m = to_perceptual(vec3<f32>(m)).x; }
    if ((P.mask_invert & 1u) != 0u) { m = 1.0 - m; }
    return clamp(m, 0.0, 1.0);
}

fn sample_layer(p: vec2<f32>) -> vec4<f32> {
    let s = P.inv0.xy * p.x + P.inv1.xy * p.y + P.inv2.xy;
    if (s.x < 0.0 || s.y < 0.0 || s.x > P.src_size.x || s.y > P.src_size.y) {
        return vec4<f32>(0.0);
    }
    let uv = s / P.src_size;
    var c = textureSampleLevel(src_tex, src_sampler, uv, 0.0);
    let blur_px = FX.wrap.z * FX.scale.x;
    if (FX.scale.y > 0.5 && blur_px > 0.25) {
        var acc = c;
        for (var i = 0u; i < TAPS; i++) { acc += sample_src(s + disk(i) * blur_px); }
        c = acc / f32(TAPS + 1u);
    } else if (FX.scale.y > 0.5 && FX.wrap.w > 0.0) {
        var acc = vec4<f32>(0.0);
        for (var i = 0u; i < 8u; i++) {
            let a = f32(i) * 0.7853982;
            acc += sample_src(s + vec2<f32>(cos(a), sin(a)) * 1.5 * max(FX.scale.x, 0.5));
        }
        c = max(c + FX.wrap.w * (c - acc / 8.0), vec4<f32>(0.0));
    }
    if (P.has_mask != 0u) { c = c * mask_value(uv); }
    return c;
}

@compute @workgroup_size(8, 8)
fn composite(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(dst);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    let px = vec2<i32>(gid.xy);
    let back = textureLoad(backdrop, px, 0);
    var pc0 = vec2<f32>(gid.xy) + vec2<f32>(0.5);
    if (FX.scale.y > 0.5 && FX.game.x >= 1.5) {
        pc0 = (floor(pc0 / FX.game.x) + vec2<f32>(0.5)) * FX.game.x;
    }
    let lay = sample_layer(pc0);

    let a_in = back.a;
    let a_l = clamp(lay.a * P.opacity, 0.0, 1.0);
    if (a_l <= 0.0) { textureStore(dst, px, back); return; }

    var l = lay.rgb / max(lay.a, 1e-6);
    if (P.has_grade != 0u) { l = grade(l); }
    if (FX.scale.y > 0.5) {
        let pc = vec2<f32>(gid.xy) + vec2<f32>(0.5);
        if (FX.relight0.w > 0.5) {
            let s = to_source(pc);
            l = relight(l, s / P.src_size);
        }
        if (FX.wrap.x > 0.0) {
            var ab = 0.0;
            var bg = vec3<f32>(0.0);
            let dimsf = vec2<f32>(dims);
            for (var i = 0u; i < TAPS; i++) {
                let q = pc + disk(i) * FX.wrap.y;
                ab += layer_alpha_at(q);
                bg += textureSampleLevel(backdrop, src_sampler, q / dimsf, 0.0).rgb;
            }
            ab /= f32(TAPS);
            bg /= f32(TAPS);
            l = l + bg * (1.0 - ab) * FX.wrap.x;
        }
        if (FX.dapple4.y > 0.5) {
            let lit = borrowed_light(pc, vec2<f32>(dims));
            let shade = mix(FX.dapple3.rgb, vec3<f32>(1.0), lit);
            l = l * mix(vec3<f32>(1.0), shade, clamp(FX.dapple3.w, 0.0, 1.0));
        }
        if (FX.haze.w > 0.0) {
            l = mix(l, FX.haze.rgb, clamp(FX.haze.w, 0.0, 1.0));
        }
        if (FX.game.y >= 2.0) {
            let lv = FX.game.y - 1.0;
            l = from_perceptual(floor(to_perceptual(l) * lv + 0.5) / lv);
        }
        if (FX.grain.x > 0.0) {
            let cell = floor(pc / max(FX.grain.y, 0.5));
            let seed = vec2<f32>(FX.grain.w * 17.0, FX.grain.w * 31.0);
            let g0 = hash2(cell + seed) + hash2(cell + seed + 71.3) - 1.0;
            var g = vec3<f32>(g0);
            if (FX.grain.z < 0.5) {
                g = vec3<f32>(g0, hash2(cell + seed + 13.7) + hash2(cell + seed + 5.1) - 1.0, hash2(cell + seed + 29.9) + hash2(cell + seed + 47.2) - 1.0);
            }
            l = from_perceptual(max(to_perceptual(l) + g * FX.grain.x, vec3<f32>(0.0)));
        }
    }
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

// ---- cast + contact shadow: darkens the backdrop with the layer's projected silhouette ----
struct ShadowParams {
    cast0: vec4<f32>,     // canvas -> source rows for the cast shadow
    cast1: vec4<f32>,
    cast2: vec4<f32>,
    contact0: vec4<f32>,  // canvas -> source rows for the contact shadow
    contact1: vec4<f32>,
    contact2: vec4<f32>,
    size: vec4<f32>,      // source w, h, cast softness (src px), contact softness (src px)
    color: vec4<f32>,     // tint rgb, cast opacity
    extra: vec4<f32>,     // contact opacity, has mask, mask invert, follow light map
};
@group(0) @binding(0) var sh_backdrop: texture_2d<f32>;
@group(0) @binding(1) var sh_dst: texture_storage_2d<rgba16float, write>;
@group(0) @binding(2) var sh_src: texture_2d<f32>;
@group(0) @binding(3) var sh_sampler: sampler;
@group(0) @binding(4) var<uniform> SH: ShadowParams;
@group(0) @binding(5) var sh_mask: texture_2d<f32>;
@group(0) @binding(6) var sh_light: texture_2d<f32>;

fn sh_alpha(s: vec2<f32>) -> f32 {
    if (s.x < 0.0 || s.y < 0.0 || s.x > SH.size.x || s.y > SH.size.y) { return 0.0; }
    let uv = s / SH.size.xy;
    var a = textureSampleLevel(sh_src, sh_sampler, uv, 0.0).a;
    if (SH.extra.y > 0.5) {
        var m = textureSampleLevel(sh_mask, sh_sampler, uv, 0.0).r;
        if (SH.extra.z > 0.5) { m = 1.0 - m; }
        a = a * clamp(m, 0.0, 1.0);
    }
    return a;
}

fn sh_soft(p: vec2<f32>, r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, radius: f32) -> f32 {
    let s = r0.xy * p.x + r1.xy * p.y + r2.xy;
    var a = sh_alpha(s);
    for (var i = 0u; i < TAPS; i++) { a += sh_alpha(s + disk(i) * radius); }
    return a / f32(TAPS + 1u);
}

@compute @workgroup_size(8, 8)
fn shadow(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(sh_dst);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    let px = vec2<i32>(gid.xy);
    let back = textureLoad(sh_backdrop, px, 0);
    let p = vec2<f32>(gid.xy) + vec2<f32>(0.5);
    var k = 0.0;
    if (SH.color.w > 0.0) { k = max(k, SH.color.w * sh_soft(p, SH.cast0, SH.cast1, SH.cast2, SH.size.z)); }
    if (SH.extra.x > 0.0) { k = max(k, SH.extra.x * sh_soft(p, SH.contact0, SH.contact1, SH.contact2, SH.size.w)); }
    if (SH.extra.w > 0.5) {
        // Ground already in the scene's shadow can't lose more sunlight.
        k = k * textureSampleLevel(sh_light, sh_sampler, p / vec2<f32>(dims), 0.0).r;
    }
    let tint = mix(vec3<f32>(1.0), SH.color.rgb, clamp(k, 0.0, 1.0));
    textureStore(sh_dst, px, vec4<f32>(back.rgb * tint, back.a));
}
