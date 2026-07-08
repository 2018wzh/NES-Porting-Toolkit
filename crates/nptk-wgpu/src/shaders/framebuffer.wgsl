struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    // Full-screen quad using triangle strip (4 vertices)
    let pos = array(
        vec2f(-1.0, -1.0),  // bottom-left
        vec2f( 1.0, -1.0),  // bottom-right
        vec2f(-1.0,  1.0),  // top-left
        vec2f( 1.0,  1.0),  // top-right
    );

    // NES aspect ratio correction (256:240 → ~8:7 or 4:3)
    // ponytail: simple stretch, add proper aspect when needed
    let uv = array(
        vec2f(0.0, 1.0),
        vec2f(1.0, 1.0),
        vec2f(0.0, 0.0),
        vec2f(1.0, 0.0),
    );

    return VertexOutput(vec4f(pos[vi], 0.0, 1.0), uv[vi]);
}

@group(0) @binding(0) var fb_texture: texture_2d<f32>;
@group(0) @binding(1) var fb_sampler: sampler;
@group(0) @binding(2) var<uniform> palette: array<vec4f, 64>;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let index = textureSampleLevel(fb_texture, fb_sampler, input.uv, 0.0).r;
    let palette_idx = u32(round(index * 255.0)) % 64;
    return palette[palette_idx];
}