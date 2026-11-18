use crate::tiles::{TEX_H, TEX_W};
use std::sync::Arc;
use winit::window::Window;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GridVertex {
    pos: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUbo {
    view_proj: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TileUbo {
    model: [[f32; 4]; 4],
    params: [f32; 4],
    accent: [f32; 4],
    secondary: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GridUbo {
    color: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompUbo {
    params: [f32; 4],
    extra: [f32; 4],
}

fn ubo(params: [f32; 4]) -> CompUbo {
    CompUbo { params, extra: [0.0; 4] }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderQuality {
    pub divisor: u32,
    pub passes: u32,
    pub bloom: bool,
    pub msaa: u32,
}

impl RenderQuality {
    pub fn parse(name: &str) -> Self {
        match name {
            "low" => Self { divisor: 8, passes: 2, bloom: false, msaa: 1 },
            "high" => Self { divisor: 2, passes: 4, bloom: true, msaa: 4 },
            _ => Self { divisor: 4, passes: 3, bloom: true, msaa: 1 },
        }
    }

    pub fn name(self) -> &'static str {
        match self.divisor {
            8 => "low",
            2 => "high",
            _ => "medium",
        }
    }
}

pub struct TileDraw<'a> {
    pub model: [[f32; 4]; 4],
    pub params: [f32; 4],
    pub accent: [f32; 4],
    pub secondary: [f32; 4],
    pub pixels: &'a [u8],
    pub upload: bool,
}

struct OwnedTex {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

struct TileRes {
    texture: wgpu::Texture,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    srgb: bool,
    premul: bool,
    transparent: bool,
    quality: RenderQuality,
    adapter_line: String,
    down_pipeline: wgpu::RenderPipeline,
    tile_overlay: wgpu::RenderPipeline,
    frost_a: OwnedTex,
    frost_b: OwnedTex,
    frame_depth: wgpu::TextureView,
    scene_msaa: Option<OwnedTex>,
    down_bg: wgpu::BindGroup,
    frost_h_bg: wgpu::BindGroup,
    frost_v_bg: wgpu::BindGroup,
    composite_sharp: wgpu::BindGroup,
    down_buf: wgpu::Buffer,
    frost_h_buf: wgpu::Buffer,
    frost_v_buf: wgpu::Buffer,
    sampler: wgpu::Sampler,
    camera_buf: wgpu::Buffer,
    camera_bg: wgpu::BindGroup,
    tile_pipeline: wgpu::RenderPipeline,
    tiles: Vec<TileRes>,
    quad_vb: wgpu::Buffer,
    quad_ib: wgpu::Buffer,
    grid_pipeline: wgpu::RenderPipeline,
    grid_vb: wgpu::Buffer,
    grid_ib: wgpu::Buffer,
    grid_buf: wgpu::Buffer,
    grid_bg: wgpu::BindGroup,
    scene: OwnedTex,
    depth_view: wgpu::TextureView,
    bloom_a: OwnedTex,
    bloom_b: OwnedTex,
    frame: OwnedTex,
    hud: OwnedTex,
    extract_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    extract_bg: wgpu::BindGroup,
    blur_h_bg: wgpu::BindGroup,
    blur_v_bg: wgpu::BindGroup,
    composite_bg: wgpu::BindGroup,
    blit_bg: wgpu::BindGroup,
    blur_h_buf: wgpu::Buffer,
    blur_v_buf: wgpu::Buffer,
    comp_buf: wgpu::Buffer,
    blit_buf: wgpu::Buffer,
    post_layouts: PostLayouts,
    width: u32,
    height: u32,
}

struct PostLayouts {
    extract: wgpu::BindGroupLayout,
    blur: wgpu::BindGroupLayout,
    composite: wgpu::BindGroupLayout,
    blit: wgpu::BindGroupLayout,
}

impl Gpu {
    pub fn new(window: Arc<Window>, tile_count: usize, opts: GpuOptions) -> Result<Self, String> {
        let boot = boot_gpu(&window, opts)?;
        let surface = boot.surface;
        let device = boot.device;
        let queue = boot.queue;
        let config = boot.config;
        let width = config.width;
        let height = config.height;
        let format = config.format;
        let mut quality = opts.quality;
        if quality.msaa > boot.max_samples || (quality.msaa > 1 && !boot.resolve) {
            quality.msaa = 1;
        }
        quality.msaa = quality.msaa.max(1);
        let msaa = quality.msaa;
        let adapter_line = format!("{} quality={} msaa={msaa}", boot.adapter_line, quality.name());
        eprintln!("{adapter_line}");
        log::info!("{adapter_line}");

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("linear"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
        });
        let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<CameraUbo>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            }],
        });

        let tile_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tile"),
            entries: &[
                texture_entry(0, wgpu::ShaderStages::FRAGMENT),
                sampler_entry(1, wgpu::ShaderStages::FRAGMENT),
                uniform_entry(2, wgpu::ShaderStages::VERTEX_FRAGMENT),
            ],
        });
        let tile_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tile"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tile.wgsl").into()),
        });
        let tile_pipeline = make_tile_pipeline(&device, &camera_layout, &tile_layout, &tile_shader, msaa);
        let tile_overlay = make_tile_pipeline(&device, &camera_layout, &tile_layout, &tile_shader, 1);

        let quad = [
            Vertex { pos: [-0.5, 0.5, 0.0], uv: [0.0, 0.0] },
            Vertex { pos: [0.5, 0.5, 0.0], uv: [1.0, 0.0] },
            Vertex { pos: [0.5, -0.5, 0.0], uv: [1.0, 1.0] },
            Vertex { pos: [-0.5, -0.5, 0.0], uv: [0.0, 1.0] },
        ];
        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let quad_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad"),
            contents: bytemuck::cast_slice(&quad),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let quad_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-i"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let mut tiles = Vec::with_capacity(tile_count);
        for i in 0..tile_count {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tile"),
                size: wgpu::Extent3d { width: TEX_W, height: TEX_H, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let uniform = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tile-ubo"),
                size: std::mem::size_of::<TileUbo>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tile"),
                layout: &tile_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                ],
            });
            let _ = i;
            tiles.push(TileRes { texture, uniform, bind });
        }

        let grid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("grid.wgsl").into()),
        });
        let grid_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grid"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::FRAGMENT)],
        });
        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("grid"),
                bind_group_layouts: &[&camera_layout, &grid_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &grid_shader,
                entry_point: Some("vs_grid"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GridVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &grid_shader,
                entry_point: Some("fs_grid"),
                targets: &[Some(color_target(
                    wgpu::TextureFormat::Rgba8Unorm,
                    Some(wgpu::BlendState::ALPHA_BLENDING),
                ))],
                compilation_options: Default::default(),
            }),
            primitive: primitive(),
            depth_stencil: Some(depth_state()),
            multisample: mesh_samples(msaa),
            multiview: None,
            cache: None,
        });
        let y = -3.4_f32;
        let gverts = [
            GridVertex { pos: [-48.0, y, -48.0] },
            GridVertex { pos: [48.0, y, -48.0] },
            GridVertex { pos: [48.0, y, 48.0] },
            GridVertex { pos: [-48.0, y, 48.0] },
        ];
        let grid_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grid"),
            contents: bytemuck::cast_slice(&gverts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let grid_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grid-i"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let grid_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grid-ubo"),
            size: std::mem::size_of::<GridUbo>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let grid_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid"),
            layout: &grid_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: grid_buf.as_entire_binding(),
            }],
        });

        let post_layouts = PostLayouts {
            extract: tex_layout(&device, false),
            blur: tex_layout(&device, true),
            composite: composite_layout(&device),
            blit: tex_layout(&device, true),
        };
        let extract_shader = shader(&device, "extract", include_str!("extract.wgsl"));
        let blur_shader = shader(&device, "blur", include_str!("blur.wgsl"));
        let composite_shader = shader(&device, "composite", include_str!("composite.wgsl"));
        let blit_shader = shader(&device, "blit", include_str!("blit.wgsl"));
        let extract_pipeline = fullscreen_pipeline(&device, &post_layouts.extract, &extract_shader, "vs_full", "fs_extract", wgpu::TextureFormat::Rgba8Unorm, None);
        let blur_pipeline = fullscreen_pipeline(&device, &post_layouts.blur, &blur_shader, "vs_full", "fs_blur", wgpu::TextureFormat::Rgba8Unorm, None);
        let composite_pipeline = fullscreen_pipeline(&device, &post_layouts.composite, &composite_shader, "vs_full", "fs_composite", wgpu::TextureFormat::Rgba8Unorm, None);
        let blit_pipeline = fullscreen_pipeline(&device, &post_layouts.blit, &blit_shader, "vs_full", "fs_blit", format, None);
        let down_shader = shader(&device, "down", include_str!("down.wgsl"));
        let down_pipeline = fullscreen_pipeline(&device, &post_layouts.blur, &down_shader, "vs_full", "fs_down", wgpu::TextureFormat::Rgba8Unorm, None);

        let blur_h_buf = uniform_buf(&device, "blur-h");
        let blur_v_buf = uniform_buf(&device, "blur-v");
        let comp_buf = uniform_buf(&device, "comp");
        let blit_buf = uniform_buf(&device, "blit");
        let down_buf = uniform_buf(&device, "down");
        let frost_h_buf = uniform_buf(&device, "frost-h");
        let frost_v_buf = uniform_buf(&device, "frost-v");

        let targets = build_targets(
            &device,
            width,
            height,
            msaa,
            quality.divisor,
            &sampler,
            &post_layouts,
            &blur_h_buf,
            &blur_v_buf,
            &comp_buf,
            &blit_buf,
            &down_buf,
            &frost_h_buf,
            &frost_v_buf,
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            srgb: format.is_srgb(),
            premul: boot.premul,
            transparent: boot.transparent,
            quality,
            adapter_line,
            down_pipeline,
            tile_overlay,
            frost_a: targets.frost_a,
            frost_b: targets.frost_b,
            frame_depth: targets.frame_depth,
            scene_msaa: targets.scene_msaa,
            down_bg: targets.down_bg,
            frost_h_bg: targets.frost_h_bg,
            frost_v_bg: targets.frost_v_bg,
            composite_sharp: targets.composite_sharp,
            down_buf,
            frost_h_buf,
            frost_v_buf,
            sampler,
            camera_buf,
            camera_bg,
            tile_pipeline,
            tiles,
            quad_vb,
            quad_ib,
            grid_pipeline,
            grid_vb,
            grid_ib,
            grid_buf,
            grid_bg,
            scene: targets.scene,
            depth_view: targets.depth_view,
            bloom_a: targets.bloom_a,
            bloom_b: targets.bloom_b,
            frame: targets.frame,
            hud: targets.hud,
            extract_pipeline,
            blur_pipeline,
            composite_pipeline,
            blit_pipeline,
            extract_bg: targets.extract_bg,
            blur_h_bg: targets.blur_h_bg,
            blur_v_bg: targets.blur_v_bg,
            composite_bg: targets.composite_bg,
            blit_bg: targets.blit_bg,
            blur_h_buf,
            blur_v_buf,
            comp_buf,
            blit_buf,
            post_layouts,
            width,
            height,
        })
    }

    pub fn adapter_line(&self) -> &str {
        &self.adapter_line
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        let targets = build_targets(
            &self.device,
            width,
            height,
            self.quality.msaa,
            self.quality.divisor,
            &self.sampler,
            &self.post_layouts,
            &self.blur_h_buf,
            &self.blur_v_buf,
            &self.comp_buf,
            &self.blit_buf,
            &self.down_buf,
            &self.frost_h_buf,
            &self.frost_v_buf,
        );
        self.scene = targets.scene;
        self.scene_msaa = targets.scene_msaa;
        self.depth_view = targets.depth_view;
        self.bloom_a = targets.bloom_a;
        self.bloom_b = targets.bloom_b;
        self.frame = targets.frame;
        self.frame_depth = targets.frame_depth;
        self.hud = targets.hud;
        self.frost_a = targets.frost_a;
        self.frost_b = targets.frost_b;
        self.extract_bg = targets.extract_bg;
        self.blur_h_bg = targets.blur_h_bg;
        self.blur_v_bg = targets.blur_v_bg;
        self.composite_bg = targets.composite_bg;
        self.composite_sharp = targets.composite_sharp;
        self.blit_bg = targets.blit_bg;
        self.down_bg = targets.down_bg;
        self.frost_h_bg = targets.frost_h_bg;
        self.frost_v_bg = targets.frost_v_bg;
    }

    pub fn render(
        &mut self,
        view_proj: [[f32; 4]; 4],
        clear: [f32; 3],
        grid_color: [f32; 4],
        tiles: &[TileDraw],
        hud_pixels: &[u8],
        capture: bool,
        intro: f32,
        overlay: bool,
        frost: f32,
        focus: Option<usize>,
        hud_upload: bool,
    ) -> Result<Option<Vec<u8>>, String> {
        if self.width == 0 || self.height == 0 {
            return Ok(None);
        }
        self.queue.write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&CameraUbo { view_proj }));
        self.queue.write_buffer(
            &self.grid_buf,
            0,
            bytemuck::bytes_of(&GridUbo { color: grid_color, extra: [0.0; 4] }),
        );
        for (tile, draw) in self.tiles.iter().zip(tiles.iter()) {
            self.queue.write_buffer(
                &tile.uniform,
                0,
                bytemuck::bytes_of(&TileUbo {
                    model: draw.model,
                    params: draw.params,
                    accent: draw.accent,
                    secondary: draw.secondary,
                }),
            );
            if draw.upload && draw.pixels.len() == (TEX_W * TEX_H * 4) as usize {
                self.queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &tile.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    draw.pixels,
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(TEX_W * 4),
                        rows_per_image: Some(TEX_H),
                    },
                    wgpu::Extent3d { width: TEX_W, height: TEX_H, depth_or_array_layers: 1 },
                );
            }
        }
        if hud_upload && hud_pixels.len() == (self.width * self.height * 4) as usize {
            self.queue.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &self.hud.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                hud_pixels,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(self.width * 4),
                    rows_per_image: Some(self.height),
                },
                wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
            );
        }
        let bw = (self.width / 4).max(1) as f32;
        let bh = (self.height / 4).max(1) as f32;
        let frost = frost.clamp(0.0, 1.0);
        let fw = (self.width / self.quality.divisor.max(1)).max(1) as f32;
        let fh = (self.height / self.quality.divisor.max(1)).max(1) as f32;
        self.queue.write_buffer(&self.blur_h_buf, 0, bytemuck::bytes_of(&ubo([1.0 / bw, 0.0, 0.0, 0.0])));
        self.queue.write_buffer(&self.blur_v_buf, 0, bytemuck::bytes_of(&ubo([0.0, 1.0 / bh, 0.0, 0.0])));
        self.queue.write_buffer(&self.down_buf, 0, bytemuck::bytes_of(&ubo([1.0 / self.width as f32, 1.0 / self.height as f32, 0.0, 0.0])));
        self.queue.write_buffer(&self.frost_h_buf, 0, bytemuck::bytes_of(&ubo([1.0 / fw, 0.0, 0.0, 0.0])));
        self.queue.write_buffer(&self.frost_v_buf, 0, bytemuck::bytes_of(&ubo([0.0, 1.0 / fh, 0.0, 0.0])));
        let clear_a = if overlay { 0.0 } else { 1.0 };
        let bloom_strength = if self.quality.bloom { 0.95 } else { 0.0 };
        self.queue.write_buffer(
            &self.comp_buf,
            0,
            bytemuck::bytes_of(&CompUbo {
                params: [
                    intro.clamp(0.0, 1.0),
                    if overlay && self.transparent { 1.0 } else { 0.0 },
                    bloom_strength,
                    if self.premul { 1.0 } else { 0.0 },
                ],
                extra: [if frost > 0.02 { frost } else { 0.0 }, 0.0, 0.0, 0.0],
            }),
        );
        self.queue.write_buffer(
            &self.blit_buf,
            0,
            bytemuck::bytes_of(&ubo([0.0, 0.0, 0.0, if self.srgb { 1.0 } else { 0.0 }])),
        );

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(None);
            }
            Err(err) => return Err(format!("surface: {err}")),
        };
        let swap_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let color_view: &wgpu::TextureView = if let Some(msaa) = &self.scene_msaa {
                &msaa.view
            } else {
                &self.scene.view
            };
            let resolve = if self.scene_msaa.is_some() { Some(&self.scene.view) } else { None };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: resolve,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear[0] as f64,
                            g: clear[1] as f64,
                            b: clear[2] as f64,
                            a: clear_a,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.camera_bg, &[]);
            pass.set_pipeline(&self.grid_pipeline);
            pass.set_bind_group(1, &self.grid_bg, &[]);
            pass.set_vertex_buffer(0, self.grid_vb.slice(..));
            pass.set_index_buffer(self.grid_ib.slice(..), wgpu::IndexFormat::Uint16);
            pass.draw_indexed(0..6, 0, 0..1);
            pass.set_pipeline(&self.tile_pipeline);
            pass.set_vertex_buffer(0, self.quad_vb.slice(..));
            pass.set_index_buffer(self.quad_ib.slice(..), wgpu::IndexFormat::Uint16);
            for tile in &self.tiles {
                pass.set_bind_group(1, &tile.bind, &[]);
                pass.draw_indexed(0..6, 0, 0..1);
            }
        }
        if self.quality.bloom {
            self.fullscreen(&mut encoder, &self.extract_pipeline, &self.extract_bg, &self.bloom_a.view);
            self.fullscreen(&mut encoder, &self.blur_pipeline, &self.blur_h_bg, &self.bloom_b.view);
            self.fullscreen(&mut encoder, &self.blur_pipeline, &self.blur_v_bg, &self.bloom_a.view);
        } else {
            clear_view(&mut encoder, &self.bloom_a.view);
        }
        if frost > 0.02 {
            self.fullscreen(&mut encoder, &self.down_pipeline, &self.down_bg, &self.frost_a.view);
            for _ in 0..self.quality.passes.max(1) {
                self.fullscreen(&mut encoder, &self.blur_pipeline, &self.frost_h_bg, &self.frost_b.view);
                self.fullscreen(&mut encoder, &self.blur_pipeline, &self.frost_v_bg, &self.frost_a.view);
            }
            self.fullscreen(&mut encoder, &self.composite_pipeline, &self.composite_bg, &self.frame.view);
            if let Some(index) = focus {
                self.draw_focus(&mut encoder, index);
            }
        } else {
            self.fullscreen(&mut encoder, &self.composite_pipeline, &self.composite_sharp, &self.frame.view);
        }
        self.fullscreen(&mut encoder, &self.blit_pipeline, &self.blit_bg, &swap_view);

        let shot = if capture {
            let padded = align256(self.width * 4);
            let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shot"),
                size: padded as u64 * self.height as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::ImageCopyTexture {
                    texture: &self.frame.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyBuffer {
                    buffer: &buf,
                    layout: wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(self.height),
                    },
                },
                wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
            );
            Some((buf, padded))
        } else {
            None
        };

        self.queue.submit(Some(encoder.finish()));
        frame.present();

        if let Some((buf, padded)) = shot {
            let slice = buf.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            self.device.poll(wgpu::Maintain::Wait);
            rx.recv().map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
            let mapped = slice.get_mapped_range();
            let mut rgba = vec![0u8; (self.width * self.height * 4) as usize];
            for row in 0..self.height as usize {
                let src = row * padded as usize;
                let dst = row * self.width as usize * 4;
                rgba[dst..dst + self.width as usize * 4]
                    .copy_from_slice(&mapped[src..src + self.width as usize * 4]);
            }
            drop(mapped);
            buf.unmap();
            return Ok(Some(rgba));
        }
        Ok(None)
    }

    fn draw_focus(&self, encoder: &mut wgpu::CommandEncoder, index: usize) {
        let Some(tile) = self.tiles.get(index) else {
            return;
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("focus"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.frame.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.frame_depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.tile_overlay);
        pass.set_bind_group(0, &self.camera_bg, &[]);
        pass.set_vertex_buffer(0, self.quad_vb.slice(..));
        pass.set_index_buffer(self.quad_ib.slice(..), wgpu::IndexFormat::Uint16);
        pass.set_bind_group(1, &tile.bind, &[]);
        pass.draw_indexed(0..6, 0, 0..1);
    }

    fn fullscreen(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind: &wgpu::BindGroup,
        view: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("full"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn align256(n: u32) -> u32 {
    (n + 255) & !255
}

fn shader(device: &wgpu::Device, label: &str, src: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(src.into()),
    })
}

fn uniform_buf(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of::<CompUbo>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn color_tex(device: &wgpu::Device, width: u32, height: u32, usage: wgpu::TextureUsages) -> OwnedTex {
    color_tex_ms(device, width, height, 1, usage)
}

fn color_tex_ms(device: &wgpu::Device, width: u32, height: u32, samples: u32, usage: wgpu::TextureUsages) -> OwnedTex {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("color"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    OwnedTex { texture, view }
}

fn depth_tex_ms(device: &wgpu::Device, width: u32, height: u32, samples: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn mesh_samples(count: u32) -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count,
        mask: !0,
        alpha_to_coverage_enabled: false,
    }
}

fn make_tile_pipeline(
    device: &wgpu::Device,
    camera_layout: &wgpu::BindGroupLayout,
    tile_layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    samples: u32,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("tile"),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tile"),
            bind_group_layouts: &[camera_layout, tile_layout],
            push_constant_ranges: &[],
        })),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_tile"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_tile"),
            targets: &[Some(color_target(
                wgpu::TextureFormat::Rgba8Unorm,
                Some(wgpu::BlendState::ALPHA_BLENDING),
            ))],
            compilation_options: Default::default(),
        }),
        primitive: primitive(),
        depth_stencil: Some(depth_state()),
        multisample: mesh_samples(samples),
        multiview: None,
        cache: None,
    })
}

struct PostTargets {
    scene: OwnedTex,
    scene_msaa: Option<OwnedTex>,
    depth_view: wgpu::TextureView,
    bloom_a: OwnedTex,
    bloom_b: OwnedTex,
    frame: OwnedTex,
    frame_depth: wgpu::TextureView,
    hud: OwnedTex,
    frost_a: OwnedTex,
    frost_b: OwnedTex,
    extract_bg: wgpu::BindGroup,
    blur_h_bg: wgpu::BindGroup,
    blur_v_bg: wgpu::BindGroup,
    composite_bg: wgpu::BindGroup,
    composite_sharp: wgpu::BindGroup,
    blit_bg: wgpu::BindGroup,
    down_bg: wgpu::BindGroup,
    frost_h_bg: wgpu::BindGroup,
    frost_v_bg: wgpu::BindGroup,
}

fn build_targets(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    msaa: u32,
    divisor: u32,
    sampler: &wgpu::Sampler,
    layouts: &PostLayouts,
    blur_h_buf: &wgpu::Buffer,
    blur_v_buf: &wgpu::Buffer,
    comp_buf: &wgpu::Buffer,
    blit_buf: &wgpu::Buffer,
    down_buf: &wgpu::Buffer,
    frost_h_buf: &wgpu::Buffer,
    frost_v_buf: &wgpu::Buffer,
) -> PostTargets {
    let scene = color_tex(
        device,
        width,
        height,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let scene_msaa = if msaa > 1 {
        Some(color_tex_ms(device, width, height, msaa, wgpu::TextureUsages::RENDER_ATTACHMENT))
    } else {
        None
    };
    let depth_view = depth_tex_ms(device, width, height, msaa);
    let bw = (width / 4).max(1);
    let bh = (height / 4).max(1);
    let bloom_a = color_tex(
        device,
        bw,
        bh,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let bloom_b = color_tex(
        device,
        bw,
        bh,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let frame = color_tex(
        device,
        width,
        height,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
    );
    let frame_depth = depth_tex_ms(device, width, height, 1);
    let hud = color_tex(device, width, height, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    let fw = (width / divisor.max(1)).max(1);
    let fh = (height / divisor.max(1)).max(1);
    let frost_a = color_tex(
        device,
        fw,
        fh,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let frost_b = color_tex(
        device,
        fw,
        fh,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let extract_bg = bind_tex(device, &layouts.extract, &scene.view, sampler, None);
    let blur_h_bg = bind_tex(device, &layouts.blur, &bloom_a.view, sampler, Some(blur_h_buf));
    let blur_v_bg = bind_tex(device, &layouts.blur, &bloom_b.view, sampler, Some(blur_v_buf));
    let down_bg = bind_tex(device, &layouts.blur, &scene.view, sampler, Some(down_buf));
    let frost_h_bg = bind_tex(device, &layouts.blur, &frost_a.view, sampler, Some(frost_h_buf));
    let frost_v_bg = bind_tex(device, &layouts.blur, &frost_b.view, sampler, Some(frost_v_buf));
    let composite_bg = bind_composite(device, &layouts.composite, &scene.view, &bloom_a.view, &hud.view, &frost_a.view, sampler, comp_buf);
    let composite_sharp = bind_composite(device, &layouts.composite, &scene.view, &bloom_a.view, &hud.view, &scene.view, sampler, comp_buf);
    let blit_bg = bind_tex(device, &layouts.blit, &frame.view, sampler, Some(blit_buf));
    PostTargets {
        scene,
        scene_msaa,
        depth_view,
        bloom_a,
        bloom_b,
        frame,
        frame_depth,
        hud,
        frost_a,
        frost_b,
        extract_bg,
        blur_h_bg,
        blur_v_bg,
        composite_bg,
        composite_sharp,
        blit_bg,
        down_bg,
        frost_h_bg,
        frost_v_bg,
    }
}

fn clear_view(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
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
}

fn primitive() -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: wgpu::PrimitiveTopology::TriangleList,
        strip_index_format: None,
        front_face: wgpu::FrontFace::Ccw,
        cull_mode: None,
        unclipped_depth: false,
        polygon_mode: wgpu::PolygonMode::Fill,
        conservative: false,
    }
}

fn depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: wgpu::TextureFormat::Depth24Plus,
        depth_write_enabled: true,
        depth_compare: wgpu::CompareFunction::Less,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

fn color_target(format: wgpu::TextureFormat, blend: Option<wgpu::BlendState>) -> wgpu::ColorTargetState {
    wgpu::ColorTargetState {
        format,
        blend,
        write_mask: wgpu::ColorWrites::ALL,
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn tex_layout(device: &wgpu::Device, with_uniform: bool) -> wgpu::BindGroupLayout {
    let mut entries = vec![
        texture_entry(0, wgpu::ShaderStages::FRAGMENT),
        sampler_entry(1, wgpu::ShaderStages::FRAGMENT),
    ];
    if with_uniform {
        entries.push(uniform_entry(2, wgpu::ShaderStages::FRAGMENT));
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("tex"),
        entries: &entries,
    })
}

fn composite_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("composite"),
        entries: &[
            texture_entry(0, wgpu::ShaderStages::FRAGMENT),
            sampler_entry(1, wgpu::ShaderStages::FRAGMENT),
            uniform_entry(2, wgpu::ShaderStages::FRAGMENT),
            texture_entry(3, wgpu::ShaderStages::FRAGMENT),
            sampler_entry(4, wgpu::ShaderStages::FRAGMENT),
            texture_entry(5, wgpu::ShaderStages::FRAGMENT),
            sampler_entry(6, wgpu::ShaderStages::FRAGMENT),
            texture_entry(7, wgpu::ShaderStages::FRAGMENT),
            sampler_entry(8, wgpu::ShaderStages::FRAGMENT),
        ],
    })
}

fn bind_tex(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform: Option<&wgpu::Buffer>,
) -> wgpu::BindGroup {
    let mut entries = vec![
        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
    ];
    if let Some(buf) = uniform {
        entries.push(wgpu::BindGroupEntry { binding: 2, resource: buf.as_entire_binding() });
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tex-bg"),
        layout,
        entries: &entries,
    })
}

fn bind_composite(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    scene: &wgpu::TextureView,
    bloom: &wgpu::TextureView,
    hud: &wgpu::TextureView,
    frost: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("comp-bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(scene) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(bloom) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(hud) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(frost) },
            wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    module: &wgpu::ShaderModule,
    vs: &str,
    fs: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(fs),
            bind_group_layouts: &[layout],
            push_constant_ranges: &[],
        })),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vs),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            targets: &[Some(color_target(format, blend))],
            compilation_options: Default::default(),
        }),
        primitive: primitive(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuBackend {
    Auto,
    Vulkan,
    Gl,
}

#[derive(Clone, Copy, Debug)]
pub struct GpuOptions {
    pub backend: GpuBackend,
    pub software: bool,
    pub want_transparency: bool,
    pub quality: RenderQuality,
}

struct Boot {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    premul: bool,
    transparent: bool,
    max_samples: u32,
    resolve: bool,
    adapter_line: String,
}

struct Attempt {
    backend: wgpu::Backends,
    software: bool,
    transparent: bool,
    label: &'static str,
}

fn boot_gpu(window: &Arc<Window>, opts: GpuOptions) -> Result<Boot, String> {
    log_gpu_env();
    let attempts = attempts_for(opts);
    let mut errors = Vec::new();
    for attempt in attempts {
        log::info!(
            "gpu try: {} backend={:?} software={} transparent={}",
            attempt.label,
            attempt.backend,
            attempt.software,
            attempt.transparent
        );
        match try_attempt(window, &attempt) {
            Ok(boot) => return Ok(boot),
            Err(err) => {
                log::warn!("gpu try {} failed: {err}", attempt.label);
                errors.push(format!("{}: {err}", attempt.label));
            }
        }
    }
    Err(format!(
        "no usable GPU adapter.\n{}\nПроверьте README: vulkan-icd-loader, nvidia-utils (RTX), vulkan-radeon/intel или vulkan-swrast и `vulkaninfo --summary`. Флаги: --backend vulkan|gl, --software.",
        errors.join("\n")
    ))
}

fn attempts_for(opts: GpuOptions) -> Vec<Attempt> {
    let mut out = Vec::new();
    let mut add = |backend, label| {
        if opts.want_transparency {
            out.push(Attempt { backend, software: false, transparent: true, label });
        }
        out.push(Attempt { backend, software: false, transparent: false, label });
    };
    if !opts.software {
        match opts.backend {
            GpuBackend::Auto | GpuBackend::Vulkan => add(wgpu::Backends::VULKAN, "vulkan"),
            GpuBackend::Gl => {}
        }
        match opts.backend {
            GpuBackend::Auto | GpuBackend::Gl => add(wgpu::Backends::GL, "gl"),
            GpuBackend::Vulkan => {}
        }
    }
    let explicit_software = opts.software;
    let no_render_node = !dri_node_present();
    if explicit_software || (no_render_node && lavapipe_icd().is_some() && std::env::var_os("VK_ICD_FILENAMES").is_none()) {
        out.push(Attempt {
            backend: wgpu::Backends::VULKAN,
            software: true,
            transparent: false,
            label: "lavapipe",
        });
        if opts.want_transparency {
            out.push(Attempt {
                backend: wgpu::Backends::VULKAN,
                software: true,
                transparent: true,
                label: "lavapipe-transparent",
            });
        }
    }
    out
}

fn try_attempt(window: &Arc<Window>, attempt: &Attempt) -> Result<Boot, String> {
    let icd_guard = SoftwareIcd::apply(attempt.software)?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: attempt.backend,
        flags: wgpu::InstanceFlags::default(),
        dx12_shader_compiler: wgpu::Dx12Compiler::default(),
        gles_minor_version: wgpu::Gles3MinorVersion::Automatic,
    });
    drop(icd_guard);
    log_adapters(&instance, attempt.backend);
    window.set_transparent(attempt.transparent);
    let surface = instance
        .create_surface(window.clone())
        .map_err(|e| format!("surface: {e}"))?;
    let adapter = pick_adapter(&instance, &surface, attempt.software)
        .ok_or_else(|| "no compatible adapter for this surface".to_string())?;
    let info = adapter.get_info();
    log::info!(
        "using adapter '{}' backend={:?} type={:?} driver={} {}",
        info.name,
        info.backend,
        info.device_type,
        info.driver,
        info.driver_info
    );
    let (device, queue) = open_device(&adapter)?;
    let caps = surface.get_capabilities(&adapter);
    if caps.formats.is_empty() {
        return Err("adapter exposes no surface formats".into());
    }
    log::info!(
        "surface caps formats={:?} alpha={:?} present={:?}",
        caps.formats,
        caps.alpha_modes,
        caps.present_modes
    );
    let format = choose_format(&caps.formats);
    let alpha_mode = choose_alpha(&caps.alpha_modes, attempt.transparent);
    let present = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Fifo]
    .into_iter()
    .find(|mode| caps.present_modes.contains(mode))
    .unwrap_or(wgpu::PresentMode::Fifo);
    let size = window.inner_size();
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: present,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);
    let format_flags = adapter.get_texture_format_features(wgpu::TextureFormat::Rgba8Unorm).flags;
    let max_samples = [4u32, 2, 1]
        .into_iter()
        .find(|count| format_flags.sample_count_supported(*count))
        .unwrap_or(1);
    let resolve = format_flags.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE);
    let adapter_line = format!(
        "GPU adapter '{}' backend={:?} type={:?} driver={} {} present={:?}",
        info.name, info.backend, info.device_type, info.driver, info.driver_info, present
    );
    log::info!(
        "surface {format:?} {alpha_mode:?} {}x{} present {present:?} transparent={} msaa_max={max_samples} resolve={resolve}",
        config.width,
        config.height,
        attempt.transparent
    );
    Ok(Boot {
        surface,
        device,
        queue,
        config,
        premul: alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied,
        transparent: attempt.transparent && alpha_mode != wgpu::CompositeAlphaMode::Opaque,
        max_samples,
        resolve,
        adapter_line,
    })
}

fn pick_adapter(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface,
    software: bool,
) -> Option<wgpu::Adapter> {
    let prefs = [
        wgpu::PowerPreference::HighPerformance,
        wgpu::PowerPreference::LowPower,
    ];
    for power_preference in prefs {
        if let Some(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
            compatible_surface: Some(surface),
            force_fallback_adapter: software,
        })) {
            return Some(adapter);
        }
    }
    None
}

fn open_device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    let mut last = String::from("request_device");
    for limits in [adapter.limits(), wgpu::Limits::downlevel_defaults()] {
        match pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("acc"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        )) {
            Ok(pair) => return Ok(pair),
            Err(err) => last = err.to_string(),
        }
    }
    Err(last)
}

fn choose_format(formats: &[wgpu::TextureFormat]) -> wgpu::TextureFormat {
    for want in [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ] {
        if formats.contains(&want) {
            return want;
        }
    }
    formats
        .iter()
        .copied()
        .find(|format| !format.is_srgb())
        .unwrap_or(formats[0])
}

fn choose_alpha(modes: &[wgpu::CompositeAlphaMode], transparent: bool) -> wgpu::CompositeAlphaMode {
    let order = if transparent {
        [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
            wgpu::CompositeAlphaMode::Opaque,
        ]
    } else {
        [
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::Inherit,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::PreMultiplied,
        ]
    };
    order
        .into_iter()
        .find(|mode| modes.contains(mode))
        .unwrap_or(modes[0])
}

fn log_adapters(instance: &wgpu::Instance, backends: wgpu::Backends) {
    let adapters = instance.enumerate_adapters(backends);
    if adapters.is_empty() {
        log::warn!("no adapters enumerated for {backends:?}");
        return;
    }
    for adapter in adapters {
        let info = adapter.get_info();
        log::info!(
            "found adapter '{}' backend={:?} type={:?} driver={} {}",
            info.name,
            info.backend,
            info.device_type,
            info.driver,
            info.driver_info
        );
    }
}

fn log_gpu_env() {
    match std::fs::read_dir("/dev/dri") {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            log::info!(
                "/dev/dri: {}",
                if names.is_empty() { "(empty)".to_string() } else { names.join(", ") }
            );
        }
        Err(err) => log::info!("/dev/dri unreadable: {err}"),
    }
    for icd in [
        "/usr/share/vulkan/icd.d/nvidia_icd.json",
        "/usr/share/vulkan/icd.d/radeon_icd.json",
        "/usr/share/vulkan/icd.d/intel_icd.json",
        "/usr/share/vulkan/icd.d/lvp_icd.json",
    ] {
        log::info!(
            "icd {} {}",
            icd,
            if std::path::Path::new(icd).exists() { "present" } else { "absent" }
        );
    }
    match std::env::var("VK_ICD_FILENAMES") {
        Ok(value) => log::info!("VK_ICD_FILENAMES is already set, leaving it unchanged: {value}"),
        Err(_) => log::info!("VK_ICD_FILENAMES is unset; the Vulkan loader chooses the ICD"),
    }
}

fn dri_node_present() -> bool {
    let Ok(entries) = std::fs::read_dir("/dev/dri") else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name.starts_with("card") || name.starts_with("renderD")
    })
}

fn lavapipe_icd() -> Option<&'static str> {
    let path = "/usr/share/vulkan/icd.d/lvp_icd.json";
    std::path::Path::new(path).exists().then_some(path)
}

struct SoftwareIcd {
    previous: Option<String>,
    active: bool,
}

impl SoftwareIcd {
    fn apply(software: bool) -> Result<Self, String> {
        if !software {
            return Ok(Self { previous: None, active: false });
        }
        if std::env::var_os("VK_ICD_FILENAMES").is_some() {
            log::info!("software render requested, but VK_ICD_FILENAMES is already set so it is kept");
            return Ok(Self { previous: None, active: false });
        }
        let Some(path) = lavapipe_icd() else {
            return Err(
                "lavapipe ICD is missing (/usr/share/vulkan/icd.d/lvp_icd.json). Install vulkan-swrast and do not point VK_ICD_FILENAMES at a GPU that is not present."
                    .into(),
            );
        };
        log::info!("trying lavapipe via VK_ICD_FILENAMES={path}");
        std::env::set_var("VK_ICD_FILENAMES", path);
        Ok(Self {
            previous: None,
            active: true,
        })
    }
}

impl Drop for SoftwareIcd {
    fn drop(&mut self) {
        if self.active {
            std::env::remove_var("VK_ICD_FILENAMES");
        }
        if let Some(previous) = self.previous.take() {
            std::env::set_var("VK_ICD_FILENAMES", previous);
        }
    }
}

trait BufferInit {
    fn create_buffer_init(&self, desc: &wgpu::util::BufferInitDescriptor) -> wgpu::Buffer;
}

impl BufferInit for wgpu::Device {
    fn create_buffer_init(&self, desc: &wgpu::util::BufferInitDescriptor) -> wgpu::Buffer {
        use wgpu::util::DeviceExt;
        DeviceExt::create_buffer_init(self, desc)
    }
}

/// Uploads a CPU pixmap to a window. Used by the dock, launcher, and settings.
pub struct FlatGpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    texture: Option<OwnedTex>,
    bind: Option<wgpu::BindGroup>,
    width: u32,
    height: u32,
    srgb: bool,
}

impl FlatGpu {
    pub fn new(window: Arc<Window>, opts: GpuOptions) -> Result<Self, String> {
        let boot = boot_gpu(&window, opts)?;
        let sampler = boot.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("flat"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let layout = tex_layout(&boot.device, true);
        let module = shader(&boot.device, "flat-blit", include_str!("blit.wgsl"));
        let pipeline = fullscreen_pipeline(
            &boot.device,
            &layout,
            &module,
            "vs_full",
            "fs_blit",
            boot.config.format,
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let uniform = boot.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("flat-ubo"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let srgb = boot.config.format.is_srgb();
        Ok(Self {
            surface: boot.surface,
            device: boot.device,
            queue: boot.queue,
            config: boot.config,
            pipeline,
            sampler,
            layout,
            uniform,
            texture: None,
            bind: None,
            width: 0,
            height: 0,
            srgb,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if width == self.config.width && height == self.config.height && self.texture.is_some() {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.texture = None;
        self.bind = None;
        self.width = 0;
        self.height = 0;
    }

    pub fn present(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
        let width = width.max(1);
        let height = height.max(1);
        self.resize(width, height);
        if self.width != width || self.height != height || self.texture.is_none() {
            let texture = color_tex(
                &self.device,
                width,
                height,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            );
            let bind = bind_tex(&self.device, &self.layout, &texture.view, &self.sampler, Some(&self.uniform));
            self.texture = Some(texture);
            self.bind = Some(bind);
            self.width = width;
            self.height = height;
        }
        let row_bytes = width * 4;
        let padded = (row_bytes + 255) & !255;
        let upload = if padded == row_bytes && rgba.len() >= (row_bytes * height) as usize {
            rgba[..(row_bytes * height) as usize].to_vec()
        } else {
            let mut packed = vec![0u8; (padded * height) as usize];
            for y in 0..height as usize {
                let src = y * width as usize * 4;
                let dst = y * padded as usize;
                let n = (width as usize * 4).min(rgba.len().saturating_sub(src));
                if n == 0 {
                    break;
                }
                packed[dst..dst + n].copy_from_slice(&rgba[src..src + n]);
            }
            packed
        };
        let Some(texture) = self.texture.as_ref() else {
            return Ok(());
        };
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &upload,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&[0.0_f32, 0.0, 0.0, if self.srgb { 1.0 } else { 0.0 }]),
        );
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(err) => return Err(format!("surface: {err}")),
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("flat") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("flat"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            if let Some(bind) = &self.bind {
                pass.set_bind_group(0, bind, &[]);
            }
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}
