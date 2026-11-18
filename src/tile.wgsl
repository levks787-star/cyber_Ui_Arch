struct Camera {
    view_proj: mat4x4<f32>,
}
struct TileU {
    model: mat4x4<f32>,
    params: vec4<f32>,
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

@fragment
fn fs_tile(in: TileVs) -> @location(0) vec4<f32> {
    let blur = tile.params.x;
    let bright = tile.params.y;
    let focus = tile.params.z;
    var col: vec4<f32>;
    if (blur < 0.25) {
        col = textureSample(tile_tex, tile_samp, in.uv);
    } else {
        let dim = textureDimensions(tile_tex);
        let texel = vec2<f32>(blur / f32(dim.x), blur / f32(dim.y));
        var acc = vec4<f32>(0.0);
        var wsum = 0.0;
        for (var y: i32 = -2; y <= 2; y++) {
            for (var x: i32 = -2; x <= 2; x++) {
                let o = vec2<f32>(f32(x), f32(y));
                let w = exp(-dot(o, o) * 0.22);
                acc += textureSample(tile_tex, tile_samp, in.uv + o * texel) * w;
                wsum += w;
            }
        }
        col = acc / max(wsum, 0.0001);
    }
    col = vec4<f32>(col.rgb * bright, 1.0);
    let edge = min(min(in.uv.x, 1.0 - in.uv.x), min(in.uv.y, 1.0 - in.uv.y));
    let glow = (1.0 - smoothstep(0.0, 0.07, edge)) * focus;
    col = vec4<f32>(col.rgb + col.rgb * glow * 0.85, 1.0);
    return col;
}
