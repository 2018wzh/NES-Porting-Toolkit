//! NES tilemap native renderer
//! Instanced rendering of background tiles from nametable + CHR data

use wgpu::util::DeviceExt;

/// Tile instance data — matches WGSL vertex shader inputs
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TileInstance {
    /// Tile top-left corner in pixel space (x = col*8, y = row*8)
    pub pos: [f32; 2],
    /// Tile index in the 16-wide CHR atlas (0-255)
    pub tile_id: u32,
    /// Palette base index into 32-entry palette: 0, 4, 8, or 12
    pub palette_id: u32,
    /// Reserved for future use (e.g. priority)
    pub flags: u32,
}

/// Quad vertex — corner offset within an 8x8 tile
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct QuadVertex {
    offset: [f32; 2],
}

const QUAD: [QuadVertex; 4] = [
    QuadVertex { offset: [0.0, 0.0] },
    QuadVertex { offset: [1.0, 0.0] },
    QuadVertex { offset: [0.0, 1.0] },
    QuadVertex { offset: [1.0, 1.0] },
];

/// Visible NES tile grid dimensions
pub const TILE_COLS: u32 = 32;
pub const TILE_ROWS: u32 = 30;
pub const MAX_TILES: usize = (TILE_COLS * TILE_ROWS) as usize; // 960

/// CHR atlas texture dimensions
const CHR_ATLAS_DIM: u32 = 128; // 16 tiles wide * 8px
const CHR_TILES_PER_ROW: u32 = 16;
const TILE_PX: u32 = 8;

pub struct TilemapRenderer {
    pub chr_atlas: wgpu::Texture,
    pub chr_atlas_view: wgpu::TextureView,
    pub chr_bind_group: wgpu::BindGroup,
    pub tile_pipeline: wgpu::RenderPipeline,
    pub quad_buffer: wgpu::Buffer,
    pub instance_buffer: wgpu::Buffer,
    pub palette_buffer: wgpu::Buffer,
    pub tile_count: u32,
}

impl TilemapRenderer {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        // --- Quad vertex buffer ---
        let quad_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Tile Quad Verts"),
            contents: bytemuck::cast_slice(&QUAD),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // --- Placeholder CHR atlas texture ---
        let chr_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("CHR Atlas"),
            size: wgpu::Extent3d {
                width: CHR_ATLAS_DIM,
                height: CHR_ATLAS_DIM,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let chr_atlas_view = chr_atlas.create_view(&wgpu::TextureViewDescriptor::default());

        // --- Palette uniform buffer (32 RGBA entries = 512 bytes) ---
        let palette_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Tilemap Palette Uniform"),
            contents: &[0u8; 512], // zeroed initially
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // --- Instance buffer (pre-allocated for MAX_TILES) ---
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Tile Instance Buffer"),
            size: (MAX_TILES * std::mem::size_of::<TileInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Sampler ---
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("CHR Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // --- Bind group layout ---
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Tilemap Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let chr_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Tilemap Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&chr_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(
                        palette_buffer.as_entire_buffer_binding(),
                    ),
                },
            ],
        });

        // --- Pipeline layout ---
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Tilemap Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // --- Shader ---
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Tilemap Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/tilemap.wgsl").into()),
        });

        // --- Render pipeline ---
        let tile_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Tilemap Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    // Buffer 0: quad corner offsets (per-vertex)
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        }],
                    },
                    // Buffer 1: tile instance data (per-instance)
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<TileInstance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 0,
                                shader_location: 1,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32,
                                offset: 8,
                                shader_location: 2,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32,
                                offset: 12,
                                shader_location: 3,
                            },
                        ],
                    },
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        TilemapRenderer {
            chr_atlas,
            chr_atlas_view,
            chr_bind_group,
            tile_pipeline,
            quad_buffer,
            instance_buffer,
            palette_buffer,
            tile_count: 0,
        }
    }

    /// Upload CHR-ROM data as a texture atlas.
    /// `chr_data` contains raw 2-bitplane tiles (16 bytes per tile).
    pub fn upload_chr(&mut self, _device: &wgpu::Device, queue: &wgpu::Queue, chr_data: &[u8]) {
        // We need at least enough data for placeholder tiles
        let num_tiles = (chr_data.len() / 16).min(256);
        if num_tiles == 0 {
            return;
        }

        // Decode CHR: each tile is 16 bytes → 64 bytes (8x8 pixels, 1 byte each)
        let atlas_bytes = decode_chr_atlas(chr_data, num_tiles);

        // Recreate the atlas texture if size changed (shouldn't, but just in case)
        // For simplicity, always write into the existing 128x128 texture
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.chr_atlas,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas_bytes,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(CHR_ATLAS_DIM),
                rows_per_image: Some(CHR_ATLAS_DIM),
            },
            wgpu::Extent3d {
                width: CHR_ATLAS_DIM,
                height: CHR_ATLAS_DIM,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Build tile instances from nametable + attribute table.
    /// `nametable`: 1024 bytes (32x32 grid, first 960 are visible)
    /// `attr`: 64 bytes of attribute table data
    /// `palette`: PPU palette RAM (32 bytes, raw NES color indices)
    pub fn build_instances(
        &mut self,
        queue: &wgpu::Queue,
        nametable: &[u8],
        attr: &[u8],
        _palette_ram: &[u8],
    ) {
        let mut instances: Vec<TileInstance> = Vec::with_capacity(MAX_TILES);

        for row in 0..TILE_ROWS {
            for col in 0..TILE_COLS {
                let nt_idx = (row * 32 + col) as usize;
                let tile_id = if nt_idx < nametable.len() {
                    nametable[nt_idx] as u32
                } else {
                    0
                };

                // Attribute table lookup
                let attr_byte_idx = ((row / 4) * 8 + (col / 4)) as usize;
                let attr_byte = if attr_byte_idx < attr.len() {
                    attr[attr_byte_idx]
                } else {
                    0
                };
                let quadrant = ((row % 4) / 2) * 2 + ((col % 4) / 2);
                let palette = ((attr_byte >> (quadrant * 2)) & 0x03) as u32;
                // Background palettes start at index 0, 4, 8, 12 in the 32-entry palette
                let palette_id = palette * 4;

                instances.push(TileInstance {
                    pos: [(col * 8) as f32, (row * 8) as f32],
                    tile_id,
                    palette_id,
                    flags: 0,
                });
            }
        }

        self.tile_count = instances.len() as u32;

        // Upload instance data to GPU buffer
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
    }

    /// Render all tile instances
    pub fn render<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>) {
        if self.tile_count == 0 {
            return;
        }

        rpass.set_pipeline(&self.tile_pipeline);
        rpass.set_bind_group(0, &self.chr_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.quad_buffer.slice(..));
        rpass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        rpass.draw(0..4, 0..self.tile_count);
    }

    pub fn update_palette(&self, queue: &wgpu::Queue, palette_ram: &[u8]) {
        let mut data = [0u8; 512]; // 32 entries * 4 f32 * 4 bytes = 512
        for (i, &idx) in palette_ram.iter().enumerate().take(32) {
            let (r, g, b) = crate::palette::NES_PALETTE[(idx as usize) % 64];
            let base = i * 16;
            // Each entry is 4 x f32 (16 bytes), stored as little-endian
            data[base..base + 4].copy_from_slice(&(r as f32 / 255.0).to_le_bytes());
            data[base + 4..base + 8].copy_from_slice(&(g as f32 / 255.0).to_le_bytes());
            data[base + 8..base + 12].copy_from_slice(&(b as f32 / 255.0).to_le_bytes());
            data[base + 12..base + 16].copy_from_slice(&(1.0f32).to_le_bytes());
        }
        queue.write_buffer(&self.palette_buffer, 0, &data);
    }
}

/// Decode raw CHR-ROM data into a flat RGBA-like atlas where each byte
/// is a 2-bit color index (0-3).  Tiles are packed in a 16-wide grid.
fn decode_chr_atlas(chr_data: &[u8], num_tiles: usize) -> Vec<u8> {
    let atlas_dim = CHR_ATLAS_DIM as usize;
    let mut atlas = vec![0u8; atlas_dim * atlas_dim];

    for t in 0..num_tiles {
        let tile_base = t * 16;
        if tile_base + 16 > chr_data.len() {
            break;
        }

        let tile_col = t % CHR_TILES_PER_ROW as usize;
        let tile_row = t / CHR_TILES_PER_ROW as usize;

        for y in 0..TILE_PX as usize {
            let plane0 = chr_data[tile_base + y];
            let plane1 = chr_data[tile_base + 8 + y];
            for x in 0..TILE_PX as usize {
                let bit = 7 - x;
                let lo = (plane0 >> bit) & 1;
                let hi = (plane1 >> bit) & 1;
                let color_idx = lo | (hi << 1); // 0..3

                let px = tile_col * TILE_PX as usize + x;
                let py = tile_row * TILE_PX as usize + y;
                atlas[py * atlas_dim + px] = color_idx;
            }
        }
    }

    atlas
}
