struct FullVs {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_full(@builtin(vertex_index) vid: u32) -> FullVs {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let p = positions[vid];
    var out: FullVs;
    out.pos = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;

@fragment
fn fs_extract(in: FullVs) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_samp, in.uv);
    let m = max(c.r, max(c.g, c.b));
    let k = smoothstep(0.45, 0.92, m);
    return vec4<f32>(c.rgb * k, 1.0);
}
