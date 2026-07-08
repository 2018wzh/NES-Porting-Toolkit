//! NES sprite native renderer
//! Instanced rendering of OAM sprites with flipping and transparency

use wgpu::util::DeviceExt;

/// Sprite instance data — matches WGSL vertex shader inputs
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteInstance {
    /// Sprite top-left corner in pixel space
    pub pos: [f32; 2],
    /// Tile index in the 16-wide CHR atlas
    pub tile_id: u32,
    /// Palette base index into 32-entry palette (16, 20, 24, or 28)
    pub palette_id: u32,
    /// Sprite priority: 0 = front, 1 = behind background (unused for now)
    pub priority: u32,
    /// Horizontal flip
    pub flip_x: u32,
    /// Vertical flip
    pub flip_y: u32,
}

/// Quad corner vertex — same layout as tilemap for shared CHR sampling
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

/// Maximum number of sprites (NES has 64 OAM entries)
pub const MAX_SPRITES: usize = 64;

pub struct SpriteRenderer {
    pub chr_atlas_view: wgpu::TextureView,
    pub chr_bind_group: wgpu::BindGroup,
    pub sprite_pipeline: wgpu::RenderPipeline,
    pub quad_buffer: wgpu::Buffer,
    pub instance_buffer: wgpu::Buffer,
    pub palette_buffer: wgpu::Buffer,
    pub sprite_count: u32,
}

impl SpriteRenderer {
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        chr_atlas: &wgpu::Texture,
    ) -> Self {
        let chr_atlas_view = chr_atlas.create_view(&wgpu::TextureViewDescriptor::default());

        // --- Quad vertex buffer (same as tilemap) ---
        let quad_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Sprite Quad Verts"),
            contents: bytemuck::cast_slice(&QUAD),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // --- Palette uniform buffer ---
        let palette_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Sprite Palette Uniform"),
            contents: &[0u8; 512],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // --- Instance buffer (MAX_SPRITES) ---
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sprite Instance Buffer"),
            size: (MAX_SPRITES * std::mem::size_of::<SpriteInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Sampler ---
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Sprite CHR Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // --- Bind group layout ---
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Sprite Bind Group Layout"),
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
            label: Some("Sprite Bind Group"),
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
            label: Some("Sprite Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // --- Shader ---
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sprite Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/sprite.wgsl").into()),
        });

        // --- Render pipeline ---
        let sprite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sprite Pipeline"),
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
                    // Buffer 1: sprite instance data (per-instance)
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<SpriteInstance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            // pos
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 0,
                                shader_location: 1,
                            },
                            // tile_id
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32,
                                offset: 8,
                                shader_location: 2,
                            },
                            // palette_id
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32,
                                offset: 12,
                                shader_location: 3,
                            },
                            // flip_x
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32,
                                offset: 20,
                                shader_location: 4,
                            },
                            // flip_y
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Uint32,
                                offset: 24,
                                shader_location: 5,
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

        SpriteRenderer {
            chr_atlas_view,
            chr_bind_group,
            sprite_pipeline,
            quad_buffer,
            instance_buffer,
            palette_buffer,
            sprite_count: 0,
        }
    }

    /// Build sprite instances from OAM data.
    /// `oam`: 256 bytes (64 sprites * 4 bytes each)
    /// `sprite_height`: 8 for 8x8 sprites, 16 for 8x16 sprites.
    pub fn build_instances(&mut self, queue: &wgpu::Queue, oam: &[u8], sprite_height: u8) {
        let mut instances: Vec<SpriteInstance> = Vec::with_capacity(MAX_SPRITES * 2);

        for i in 0..MAX_SPRITES {
            let base = i * 4;
            if base + 4 > oam.len() {
                break;
            }

            let y = oam[base] as f32;
            let tile_id = oam[base + 1] as u32;
            let attr = oam[base + 2];
            let x = oam[base + 3] as f32;

            let sprite_palette = (attr & 0x03) as u32;
            let palette_id = 16 + sprite_palette * 4;
            let priority = ((attr >> 5) & 1) as u32;
            let flip_x = ((attr >> 6) & 1) as u32;
            let flip_y = ((attr >> 7) & 1) as u32;

            let px = x;
            let py = y + 1.0;

            if py >= 240.0 || py < -8.0 || px >= 256.0 || px < -8.0 {
                continue;
            }

            instances.push(SpriteInstance {
                pos: [px, py],
                tile_id,
                palette_id,
                priority,
                flip_x,
                flip_y,
            });

            // 8x16 sprite: split into two 8x8 tiles
            if sprite_height == 16 {
                let upper_tile = tile_id & 0xFE;
                let lower_tile = tile_id | 0x01;

                instances.push(SpriteInstance {
                    pos: [px, py],
                    tile_id: upper_tile,
                    palette_id,
                    priority,
                    flip_x,
                    flip_y,
                });

                instances.push(SpriteInstance {
                    pos: [px, py + 8.0],
                    tile_id: lower_tile,
                    palette_id,
                    priority,
                    flip_x,
                    flip_y,
                });
            }
        }

        self.sprite_count = instances.len() as u32;

        if !instances.is_empty() {
            queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));
        }
    }

    /// Render all sprite instances
    pub fn render<'a>(&'a self, rpass: &mut wgpu::RenderPass<'a>) {
        if self.sprite_count == 0 {
            return;
        }

        rpass.set_pipeline(&self.sprite_pipeline);
        rpass.set_bind_group(0, &self.chr_bind_group, &[]);
        rpass.set_vertex_buffer(0, self.quad_buffer.slice(..));
        rpass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        rpass.draw(0..4, 0..self.sprite_count);
    }

    pub fn update_palette(&self, queue: &wgpu::Queue, palette_ram: &[u8]) {
        let mut data = [0u8; 512];
        for (i, &idx) in palette_ram.iter().enumerate().take(32) {
            let (r, g, b) = crate::palette::NES_PALETTE[(idx as usize) % 64];
            let base = i * 16;
            data[base..base + 4].copy_from_slice(&(r as f32 / 255.0).to_le_bytes());
            data[base + 4..base + 8].copy_from_slice(&(g as f32 / 255.0).to_le_bytes());
            data[base + 8..base + 12].copy_from_slice(&(b as f32 / 255.0).to_le_bytes());
            data[base + 12..base + 16].copy_from_slice(&(1.0f32).to_le_bytes());
        }
        queue.write_buffer(&self.palette_buffer, 0, &data);
    }
}
