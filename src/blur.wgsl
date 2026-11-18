struct CompU {
    params: vec4<f32>,
}

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
@group(0) @binding(2) var<uniform> comp: CompU;

@fragment
fn fs_blur(in: FullVs) -> @location(0) vec4<f32> {
    let texel = vec2<f32>(comp.params.x, comp.params.y);
    var acc = textureSample(src_tex, src_samp, in.uv) * 0.227027;
    acc += textureSample(src_tex, src_samp, in.uv + texel * 1.384615) * 0.316216;
    acc += textureSample(src_tex, src_samp, in.uv - texel * 1.384615) * 0.316216;
    acc += textureSample(src_tex, src_samp, in.uv + texel * 3.230769) * 0.070270;
    acc += textureSample(src_tex, src_samp, in.uv - texel * 3.230769) * 0.070270;
    return acc;
}
