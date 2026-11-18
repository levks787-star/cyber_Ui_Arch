struct Camera {
    view_proj: mat4x4<f32>,
}
struct GridU {
    color: vec4<f32>,
    extra: vec4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> grid: GridU;

struct GridVs {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
}

@vertex
fn vs_grid(@location(0) pos: vec3<f32>) -> GridVs {
    var out: GridVs;
    out.clip = camera.view_proj * vec4<f32>(pos, 1.0);
    out.world = pos;
    return out;
}

@fragment
fn fs_grid(in: GridVs) -> @location(0) vec4<f32> {
    let scale = 0.72;
    let fx = abs(fract(in.world.x * scale) - 0.5);
    let fz = abs(fract(in.world.z * scale) - 0.5);
    let line = min(fx, fz);
    let mx = abs(fract(in.world.x * scale * 0.2) - 0.5);
    let mz = abs(fract(in.world.z * scale * 0.2) - 0.5);
    let major = min(mx, mz);
    let a = smoothstep(0.018, 0.0, line) * 0.55 + smoothstep(0.028, 0.0, major) * 0.9;
    let dist = length(in.world.xz);
    let fade = exp(-dist * 0.055);
    let intensity = clamp(a * fade, 0.0, 1.0);
    return vec4<f32>(grid.color.rgb, intensity);
}
