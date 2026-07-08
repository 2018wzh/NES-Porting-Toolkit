// NES Sprite shader — instanced sprite rendering from CHR atlas
// Supports 8x8 sprites, H/V flip, and per-pixel transparency (idx 0 = discard)

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) chr_uv: vec2<f32>,
    @location(1) @interpolate(flat) palette_base: u32,
}

const HALF_W: f32 = 128.0;
const HALF_H: f32 = 120.0;
const ATLAS_SIZE: f32 = 128.0;
const TILE_SIZE: f32 = 8.0;

@group(0) @binding(0) var chr_texture: texture_2d<f32>;
@group(0) @binding(1) var chr_sampler: sampler;
@group(0) @binding(2) var<uniform> palette: array<vec4<f32>, 32>;

@vertex
fn vs_main(
    @location(0) corner: vec2<f32>,        // quad corner offset from vertex buf
    @location(1) pos: vec2<f32>,            // sprite top-left pixel pos from instance buf
    @location(2) tile_id: u32,              // tile index in CHR atlas
    @location(3) palette_id: u32,           // palette base index (16,20,24,28 for sprites)
    @location(4) flip_x: u32,
    @location(5) flip_y: u32,
) -> VertexOutput {
    var c = corner;

    // Apply flips by mirroring the corner offset
    if flip_x != 0u { c.x = 1.0 - c.x; }
    if flip_y != 0u { c.y = 1.0 - c.y; }

    // Pixel-space position
    let pixel_x = pos.x + c.x * TILE_SIZE;
    let pixel_y = pos.y + c.y * TILE_SIZE;

    // NDC
    let ndc_x = pixel_x / HALF_W - 1.0;
    let ndc_y = 1.0 - pixel_y / HALF_H;

    // CHR atlas UV
    let tile_col = tile_id % 16u;
    let tile_row = tile_id / 16u;
    let chr_u = (f32(tile_col) * TILE_SIZE + c.x * TILE_SIZE) / ATLAS_SIZE;
    let chr_v = (f32(tile_row) * TILE_SIZE + c.y * TILE_SIZE) / ATLAS_SIZE;

    return VertexOutput(
        vec4(ndc_x, ndc_y, 0.0, 1.0),
        vec2(chr_u, chr_v),
        palette_id,
    );
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let raw = textureSample(chr_texture, chr_sampler, input.chr_uv).r;
    let pixel = u32(round(raw * 255.0));

    // Pixel value 0 = transparent (discard)
    if pixel == 0u {
        discard;
    }

    let idx = input.palette_base + pixel;
    return palette[idx % 32u];
}
