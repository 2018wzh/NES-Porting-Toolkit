// NES Tilemap shader — instanced tile rendering from CHR atlas
// ponytail: hardcoded 8x8 tile size, 16-wide atlas grid, 32-entry palette

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) chr_uv: vec2<f32>,
    @location(1) @interpolate(flat) palette_base: u32,
}

// NES screen: 256x240 pixels; NDC half-extents in pixel space
const HALF_W: f32 = 128.0;  // 256 / 2
const HALF_H: f32 = 120.0;  // 240 / 2
const ATLAS_SIZE: f32 = 128.0;    // 16x16 tiles * 8px
const TILE_SIZE: f32 = 8.0;

@group(0) @binding(0) var chr_texture: texture_2d<f32>;
@group(0) @binding(1) var chr_sampler: sampler;
@group(0) @binding(2) var<uniform> palette: array<vec4<f32>, 32>;

@vertex
fn vs_main(
    @location(0) corner: vec2<f32>,       // quad corner offset from vertex buf
    @location(1) pos: vec2<f32>,           // tile top-left pixel pos from instance buf
    @location(2) tile_id: u32,             // tile index in CHR atlas (0-255)
    @location(3) palette_id: u32,          // palette base index (0,4,8,12)
) -> VertexOutput {
    // Pixel-space position of this corner
    let pixel_x = pos.x + corner.x * TILE_SIZE;
    let pixel_y = pos.y + corner.y * TILE_SIZE;

    // NDC: x in [-1,1], y in [-1,1] (Y-up in wgpu)
    let ndc_x = pixel_x / HALF_W - 1.0;
    let ndc_y = 1.0 - pixel_y / HALF_H;

    // CHR atlas UV — tiles laid out 16-wide in the atlas
    let tile_col = tile_id % 16u;
    let tile_row = tile_id / 16u;
    let chr_u = (f32(tile_col) * TILE_SIZE + corner.x * TILE_SIZE) / ATLAS_SIZE;
    let chr_v = (f32(tile_row) * TILE_SIZE + corner.y * TILE_SIZE) / ATLAS_SIZE;

    return VertexOutput(
        vec4(ndc_x, ndc_y, 0.0, 1.0),
        vec2(chr_u, chr_v),
        palette_id,
    );
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Sample CHR atlas; each texel is 0..3 stored as R8Unorm
    let raw = textureSample(chr_texture, chr_sampler, input.chr_uv).r;
    let pixel = u32(round(raw * 255.0));
    let idx = input.palette_base + pixel;
    return palette[idx % 32u];
}
