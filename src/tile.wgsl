struct Camera {
    view_proj: mat4x4<f32>,
}
struct TileU {
    model: mat4x4<f32>,
    params: vec4<f32>,
    accent: vec4<f32>,
    secondary: vec4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var tile_tex: texture_2d<f32>;
@group(1) @binding(1) var tile_samp: sampler;
@group(1) @binding(2) var<uniform> tile: TileU;

struct TileVs {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_tile(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>) -> TileVs {
    var out: TileVs;
    out.clip = camera.view_proj * tile.model * vec4<f32>(pos, 1.0);
    out.uv = uv;
    return out;
}

fn tile_dist(uv: vec2<f32>) -> f32 {
    let dim = vec2<f32>(textureDimensions(tile_tex));
    let p = uv * dim;
    let radius = 13.0;
    let half = dim * 0.5;
    let q = abs(p - half) - (half - vec2<f32>(radius));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn grain(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

@fragment
fn fs_tile(in: TileVs) -> @location(0) vec4<f32> {
    let frost = tile.params.w;
    let bright = tile.params.y;
    let focus = tile.params.z;
    var rgb = textureSample(tile_tex, tile_samp, in.uv).rgb;
    let luma = dot(rgb, vec3<f32>(0.22, 0.67, 0.11));
    rgb = mix(rgb, vec3<f32>(luma), frost * 0.72);
    rgb *= mix(1.0, 0.86, frost);
    let px = floor(in.uv * vec2<f32>(textureDimensions(tile_tex)));
    rgb += (grain(px) - 0.5) * frost * 0.055;
    rgb *= bright;

    let dist = tile_dist(in.uv);
    let alpha = 1.0 - smoothstep(-0.15, 0.9, dist);
    let ring = (1.0 - smoothstep(0.2, 1.15, abs(dist))) * step(dist, 0.55);
    let edge_col = mix(tile.accent.rgb, tile.secondary.rgb, focus * 0.85);
    let edge_a = mix(tile.accent.a, 0.95, focus);
    rgb = mix(rgb, edge_col, ring * edge_a);
    return vec4<f32>(rgb, alpha);
}
