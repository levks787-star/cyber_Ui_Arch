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
}

pub struct TileDraw<'a> {
    pub model: [[f32; 4]; 4],
    pub params: [f32; 4],
    pub accent: [f32; 4],
    pub secondary: [f32; 4],
    pub pixels: &'a [u8],
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
    pub fn new(window: Arc<Window>, tile_count: usize) -> Result<Self, String> {
        if !std::path::Path::new("/dev/dri/card0").exists() && std::env::var_os("VK_ICD_FILENAMES").is_none()
        {
            log::info!("no DRM card, forcing lavapipe");
            std::env::set_var(
                "VK_ICD_FILENAMES",
                "/usr/share/vulkan/icd.d/lvp_icd.json",
            );
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            flags: wgpu::InstanceFlags::default(),
            dx12_shader_compiler: wgpu::Dx12Compiler::default(),
            gles_minor_version: wgpu::Gles3MinorVersion::Automatic,
        });
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("surface: {e}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .or_else(|| {
            log::warn!("no hardware adapter, requesting fallback");
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: true,
            }))
        })
        .ok_or_else(|| "no GPU adapter. Install mesa / vulkan-swrast (lavapipe).".to_string())?;
        let info = adapter.get_info();
        log::info!("GPU {} ({:?})", info.name, info.backend);
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("acc"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| format!("device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let present = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo]
            .into_iter()
            .find(|m| caps.present_modes.contains(m))
            .unwrap_or(wgpu::PresentMode::Fifo);
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let alpha_mode = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
            wgpu::CompositeAlphaMode::Opaque,
        ]
        .into_iter()
        .find(|mode| caps.alpha_modes.contains(mode))
        .unwrap_or(caps.alpha_modes[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: present,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        log::info!("surface {format:?} {alpha_mode:?} {width}x{height} present {present:?}");

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
        let tile_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tile"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("tile"),
                bind_group_layouts: &[&camera_layout, &tile_layout],
                push_constant_ranges: &[],
            })),
            vertex: wgpu::VertexState {
                module: &tile_shader,
                entry_point: Some("vs_tile"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &tile_shader,
                entry_point: Some("fs_tile"),
                targets: &[Some(color_target(
                    wgpu::TextureFormat::Rgba8Unorm,
                    Some(wgpu::BlendState::ALPHA_BLENDING),
                ))],
                compilation_options: Default::default(),
            }),
            primitive: primitive(),
            depth_stencil: Some(depth_state()),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

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
            multisample: wgpu::MultisampleState::default(),
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

        let blur_h_buf = uniform_buf(&device, "blur-h");
        let blur_v_buf = uniform_buf(&device, "blur-v");
        let comp_buf = uniform_buf(&device, "comp");
        let blit_buf = uniform_buf(&device, "blit");

        let scene = color_tex(&device, width, height, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let depth_view = depth_tex(&device, width, height);
        let bw = (width / 4).max(1);
        let bh = (height / 4).max(1);
        let bloom_a = color_tex(&device, bw, bh, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let bloom_b = color_tex(&device, bw, bh, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        let frame = color_tex(&device, width, height, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let hud = color_tex(&device, width, height, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);

        let extract_bg = bind_tex(&device, &post_layouts.extract, &scene.view, &sampler, None);
        let blur_h_bg = bind_tex(&device, &post_layouts.blur, &bloom_a.view, &sampler, Some(&blur_h_buf));
        let blur_v_bg = bind_tex(&device, &post_layouts.blur, &bloom_b.view, &sampler, Some(&blur_v_buf));
        let composite_bg = bind_composite(&device, &post_layouts.composite, &scene.view, &bloom_a.view, &hud.view, &sampler, &comp_buf);
        let blit_bg = bind_tex(&device, &post_layouts.blit, &frame.view, &sampler, Some(&blit_buf));

        Ok(Self {
            surface,
            device,
            queue,
            config,
            srgb: format.is_srgb(),
            premul: alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied,
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
            scene,
            depth_view,
            bloom_a,
            bloom_b,
            frame,
            hud,
            extract_pipeline,
            blur_pipeline,
            composite_pipeline,
            blit_pipeline,
            extract_bg,
            blur_h_bg,
            blur_v_bg,
            composite_bg,
            blit_bg,
            blur_h_buf,
            blur_v_buf,
            comp_buf,
            blit_buf,
            post_layouts,
            width,
            height,
        })
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
        self.scene = color_tex(&self.device, width, height, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        self.depth_view = depth_tex(&self.device, width, height);
        let bw = (width / 4).max(1);
        let bh = (height / 4).max(1);
        self.bloom_a = color_tex(&self.device, bw, bh, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        self.bloom_b = color_tex(&self.device, bw, bh, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
        self.frame = color_tex(&self.device, width, height, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        self.hud = color_tex(&self.device, width, height, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        self.extract_bg = bind_tex(&self.device, &self.post_layouts.extract, &self.scene.view, &self.sampler, None);
        self.blur_h_bg = bind_tex(&self.device, &self.post_layouts.blur, &self.bloom_a.view, &self.sampler, Some(&self.blur_h_buf));
        self.blur_v_bg = bind_tex(&self.device, &self.post_layouts.blur, &self.bloom_b.view, &self.sampler, Some(&self.blur_v_buf));
        self.composite_bg = bind_composite(&self.device, &self.post_layouts.composite, &self.scene.view, &self.bloom_a.view, &self.hud.view, &self.sampler, &self.comp_buf);
        self.blit_bg = bind_tex(&self.device, &self.post_layouts.blit, &self.frame.view, &self.sampler, Some(&self.blit_buf));
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
            if draw.pixels.len() == (TEX_W * TEX_H * 4) as usize {
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
        if hud_pixels.len() == (self.width * self.height * 4) as usize {
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
        self.queue.write_buffer(&self.blur_h_buf, 0, bytemuck::bytes_of(&CompUbo { params: [1.0 / bw, 0.0, 0.0, 0.0] }));
        self.queue.write_buffer(&self.blur_v_buf, 0, bytemuck::bytes_of(&CompUbo { params: [0.0, 1.0 / bh, 0.0, 0.0] }));
        let clear_a = if overlay { 0.0 } else { 1.0 };
        self.queue.write_buffer(
            &self.comp_buf,
            0,
            bytemuck::bytes_of(&CompUbo {
                params: [
                    intro.clamp(0.0, 1.0),
                    if overlay { 1.0 } else { 0.0 },
                    0.95,
                    if self.premul { 1.0 } else { 0.0 },
                ],
            }),
        );
        self.queue.write_buffer(
            &self.blit_buf,
            0,
            bytemuck::bytes_of(&CompUbo { params: [0.0, 0.0, 0.0, if self.srgb { 1.0 } else { 0.0 }] }),
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
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.scene.view,
                    resolve_target: None,
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
        self.fullscreen(&mut encoder, &self.extract_pipeline, &self.extract_bg, &self.bloom_a.view);
        self.fullscreen(&mut encoder, &self.blur_pipeline, &self.blur_h_bg, &self.bloom_b.view);
        self.fullscreen(&mut encoder, &self.blur_pipeline, &self.blur_v_bg, &self.bloom_a.view);
        self.fullscreen(&mut encoder, &self.composite_pipeline, &self.composite_bg, &self.frame.view);
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
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("color"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    OwnedTex { texture, view }
}

fn depth_tex(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
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

trait BufferInit {
    fn create_buffer_init(&self, desc: &wgpu::util::BufferInitDescriptor) -> wgpu::Buffer;
}

impl BufferInit for wgpu::Device {
    fn create_buffer_init(&self, desc: &wgpu::util::BufferInitDescriptor) -> wgpu::Buffer {
        use wgpu::util::DeviceExt;
        DeviceExt::create_buffer_init(self, desc)
    }
}
