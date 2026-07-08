//! WGPU renderer — supports both framebuffer-upload (compat) and
//! native tilemap+sprite (original) rendering modes

use wgpu::util::DeviceExt;

use crate::palette::NES_PALETTE;
use crate::sprite::SpriteRenderer;
use crate::tilemap::TilemapRenderer;

/// Which rendering path to use
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Upload finished 256x240 framebuffer and palette-index it (slow compat)
    Framebuffer,
    /// Native: tilemap + sprite instanced rendering from CHR + nametable + OAM
    Native,
}

pub fn create_nes_palette_buffer() -> Vec<u8> {
    let mut data = Vec::with_capacity(64 * 16);
    for &(r, g, b) in NES_PALETTE.iter() {
        // WGSL array<vec4f, 64> — each vec4f is 4xf32 = 16 bytes
        data.extend_from_slice(&bytemuck::bytes_of(&[
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            1.0f32,
        ]));
    }
    data
}

pub struct WgpuRenderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    size: (u32, u32),

    // --- Framebuffer-upload mode ---
    fb_texture: wgpu::Texture,
    pub fb_bind_group: wgpu::BindGroup,
    pub fb_pipeline: wgpu::RenderPipeline,
    palette_buffer: wgpu::Buffer,
    /// Dummy vertex buffer for pipelines that use built-in vertex data
    pub dummy_vb: wgpu::Buffer,

    // --- Native rendering mode (pub for direct render-pass access) ---
    pub render_mode: RenderMode,
    pub tilemap: TilemapRenderer,
    pub sprite: SpriteRenderer,
}

impl WgpuRenderer {
    pub async fn new(
        window: &winit::window::Window,
        width: u32,
        height: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let size = (width, height);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let surface: wgpu::Surface<'static> =
            unsafe { std::mem::transmute(instance.create_surface(window)?) };
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("No GPU adapter found")?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps.formats[0];
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // --- Framebuffer mode resources ---
        let fb_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("NES Framebuffer"),
            size: wgpu::Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let fb_view = fb_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let palette_data = create_nes_palette_buffer();
        let palette_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("NES Palette"),
            contents: &palette_data,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("NES Sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let fb_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("NES FB Bind Group Layout"),
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

        let fb_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("NES FB Bind Group"),
            layout: &fb_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&fb_view),
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

        let fb_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("NES FB Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/framebuffer.wgsl").into()),
        });

        let fb_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("NES FB Pipeline Layout"),
            bind_group_layouts: &[&fb_bind_group_layout],
            push_constant_ranges: &[],
        });

        let fb_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("NES FB Pipeline"),
            layout: Some(&fb_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &fb_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 0,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &fb_shader,
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

        // --- Dummy vertex buffer (for builtin-vertex pipelines) ---
        let dummy_vb = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Dummy VB"),
            size: 4,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        // --- Native-mode renderers ---
        let tilemap = TilemapRenderer::new(&device, surface_format);
        let sprite = SpriteRenderer::new(&device, surface_format, &tilemap.chr_atlas);

        Ok(WgpuRenderer {
            surface,
            device,
            queue,
            config,
            size,
            fb_texture,
            fb_bind_group,
            fb_pipeline,
            palette_buffer,
            dummy_vb,
            render_mode: RenderMode::Framebuffer,
            tilemap,
            sprite,
        })
    }

    // ── Framebuffer mode ──────────────────────────────────────────

    pub fn upload_framebuffer(&self, fb: &[u8; 256 * 240]) {
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.fb_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            fb,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(240),
            },
            wgpu::Extent3d {
                width: 256,
                height: 240,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn update_palette(&self, palette: &[u8; 32]) {
        let mut rgba = [0u8; 256];
        for (i, &nes_idx) in palette.iter().enumerate() {
            let (r, g, b) = NES_PALETTE[nes_idx as usize % 64];
            rgba[i * 4..i * 4 + 4].copy_from_slice(&[r, g, b, 255]);
        }
        self.queue
            .write_buffer(&self.palette_buffer, 0, bytemuck::cast_slice(&rgba));
    }

    // ── Native mode ───────────────────────────────────────────────

    /// Upload NES PPU state to GPU buffers for native rendering.
    ///
    /// Call this once per frame BEFORE beginning the render pass.
    /// Then call `tilemap.render()` and `sprite.render()` inside the render pass.
    ///
    /// `chr_data` - raw CHR-ROM (2-bitplane tiles, 16 bytes each)
    /// `nametable` - 1024-byte nametable (first 960 = tile indices)
    /// `attr` - 64-byte attribute table
    /// `palette` - 32-byte PPU palette RAM (raw NES color indices)
    /// `oam` - 256-byte OAM data (64 sprites * 4 bytes)
    /// `ppu_ctrl` - PPU control register (bit 5 = 8x16 sprite mode)
    pub fn upload_native_data(
        &mut self,
        chr_data: &[u8],
        nametable: &[u8],
        attr: &[u8],
        palette: &[u8],
        oam: &[u8],
        ppu_ctrl: u8,
    ) {
        self.render_mode = RenderMode::Native;

        // Upload CHR to shared atlas texture
        self.tilemap.upload_chr(&self.device, &self.queue, chr_data);

        // Update palette on both renderers
        self.tilemap.update_palette(&self.queue, palette);
        self.sprite.update_palette(&self.queue, palette);

        // Build tile instances
        self.tilemap
            .build_instances(&self.queue, nametable, attr, palette);

        // Build sprite instances (8x16 mode from PPU ctrl bit 5)
        let sprite_height = if ppu_ctrl & 0x20 != 0 { 16 } else { 8 };
        self.sprite.build_instances(&self.queue, oam, sprite_height);
    }

    // ── Shared render dispatch ────────────────────────────────────

    pub fn render(&self) -> Result<(), Box<dyn std::error::Error>> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("NES Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            match self.render_mode {
                RenderMode::Framebuffer => {
                    rpass.set_pipeline(&self.fb_pipeline);
                    rpass.set_bind_group(0, &self.fb_bind_group, &[]);
                    rpass.draw(0..4, 0..1);
                }
                RenderMode::Native => {
                    // Draw background tiles first, then sprites on top
                    self.tilemap.render(&mut rpass);
                    self.sprite.render(&mut rpass);
                }
            }
        }

        self.queue.submit([encoder.finish()]);
        output.present();
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.size = (width, height);
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }
}
