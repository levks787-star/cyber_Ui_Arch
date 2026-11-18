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
@group(0) @binding(3) var bloom_tex: texture_2d<f32>;
@group(0) @binding(4) var bloom_samp: sampler;
@group(0) @binding(5) var hud_tex: texture_2d<f32>;
@group(0) @binding(6) var hud_samp: sampler;

@fragment
fn fs_composite(in: FullVs) -> @location(0) vec4<f32> {
    let scene = textureSample(src_tex, src_samp, in.uv);
    let bloom = textureSample(bloom_tex, bloom_samp, in.uv);
    var rgb = scene.rgb + bloom.rgb * comp.params.z;
    let vig = length(in.uv - vec2<f32>(0.5, 0.5));
    rgb *= mix(1.0, 0.62, smoothstep(0.28, 0.95, vig));
    let scan = 1.0 - 0.035 * step(0.65, fract(in.pos.y * 0.35));
    rgb *= scan;
    let hud = textureSample(hud_tex, hud_samp, in.uv);
    rgb = mix(rgb, hud.rgb, hud.a);
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let intro = comp.params.x;
    let overlay = comp.params.y;
    let veil = 0.48 * overlay * intro;
    let faded = max(scene.a, veil) * intro;
    let alpha = mix(1.0, faded, step(0.5, overlay));
    if (comp.params.w > 0.5) {
        rgb *= alpha;
    }
    return vec4<f32>(rgb, alpha);
}
