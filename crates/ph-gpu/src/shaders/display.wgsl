// Working space (linear premultiplied rgba16float) -> sRGB rgba8unorm for the egui viewer.
// Transparent regions are shown over a grey checkerboard.

struct DisplayParams { checker_size: f32, exposure: f32, _pad: vec2<f32> };

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var out_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> D: DisplayParams;

fn encode_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}

@compute @workgroup_size(8, 8)
fn display(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(out_tex);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    let c = textureLoad(src, vec2<i32>(gid.xy), 0);
    let cell = vec2<u32>(vec2<f32>(gid.xy) / D.checker_size);
    let checker = select(0.18, 0.30, ((cell.x + cell.y) & 1u) == 0u);
    let lin = c.rgb * D.exposure + vec3<f32>(checker) * (1.0 - c.a);
    textureStore(out_tex, vec2<i32>(gid.xy), vec4<f32>(encode_srgb(lin), 1.0));
}

// ---- scope downsample: working space -> small sRGB rgba8 image for CPU-side scopes ----
@group(0) @binding(0) var scope_src: texture_2d<f32>;
@group(0) @binding(1) var scope_dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var scope_sampler: sampler;

@compute @workgroup_size(8, 8)
fn scope_downsample(@builtin(global_invocation_id) gid: vec3<u32>) {
    let dims = textureDimensions(scope_dst);
    if (gid.x >= dims.x || gid.y >= dims.y) { return; }
    let uv = (vec2<f32>(gid.xy) + vec2<f32>(0.5)) / vec2<f32>(dims);
    let c = textureSampleLevel(scope_src, scope_sampler, uv, 0.0);
    let straight = c.rgb / max(c.a, 1e-6);
    textureStore(scope_dst, vec2<i32>(gid.xy), vec4<f32>(encode_srgb(straight), c.a));
}
