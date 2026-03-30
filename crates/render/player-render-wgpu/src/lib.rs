use std::sync::Arc;

use anyhow::{Context, Result};
use winit::dpi::PhysicalSize;
use winit::window::{Window, WindowAttributes};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSurfaceConfig {
    pub width: u32,
    pub height: u32,
}

impl Default for RenderSurfaceConfig {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Fit,
    Fill,
    Stretch,
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::Fit
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct RgbaVideoFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Yuv420pVideoFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum VideoFrameTexture {
    Rgba(RgbaVideoFrame),
    Yuv420p(Yuv420pVideoFrame),
}

#[derive(Debug, Clone)]
pub struct RgbaOverlayFrame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

pub fn default_window_attributes(config: RenderSurfaceConfig) -> WindowAttributes {
    Window::default_attributes()
        .with_title("Vesper basic player")
        .with_inner_size(PhysicalSize::new(config.width, config.height))
}

pub fn preferred_backends() -> wgpu::Backends {
    wgpu::Backends::METAL
}

pub struct VideoRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    rgba_texture_bind_group_layout: wgpu::BindGroupLayout,
    yuv420p_texture_bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    video_rgba_pipeline: wgpu::RenderPipeline,
    video_yuv420p_pipeline: wgpu::RenderPipeline,
    overlay_pipeline: wgpu::RenderPipeline,
    video_binding: UploadedVideoFrame,
    overlay_bind_group: Option<wgpu::BindGroup>,
    overlay_texture: Option<wgpu::Texture>,
    overlay_texture_size: Option<wgpu::Extent3d>,
    video_frame_size: (u32, u32),
    render_mode: RenderMode,
}

enum UploadedVideoFrame {
    Rgba {
        bind_group: wgpu::BindGroup,
        texture: wgpu::Texture,
        texture_size: wgpu::Extent3d,
    },
    Yuv420p {
        bind_group: wgpu::BindGroup,
        y_texture: wgpu::Texture,
        u_texture: wgpu::Texture,
        v_texture: wgpu::Texture,
        y_size: wgpu::Extent3d,
        uv_size: wgpu::Extent3d,
    },
}

impl VideoRenderer {
    pub async fn new(window: Arc<Window>, frame_size: (u32, u32)) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: preferred_backends(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance
            .create_surface(window.clone())
            .context("failed to create wgpu surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .context("failed to request a Metal adapter")?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Vesper device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .context("failed to request a wgpu device")?;

        let initial_size = window.inner_size();
        let mut config = surface
            .get_default_config(
                &adapter,
                initial_size.width.max(1),
                initial_size.height.max(1),
            )
            .context("surface does not support default configuration")?;
        config.format = preferred_surface_format(surface.get_capabilities(&adapter).formats)
            .unwrap_or(config.format);
        surface.configure(&device, &config);

        let rgba_texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("rgba texture bind group layout"),
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
                ],
            });

        let yuv420p_texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("yuv420p texture bind group layout"),
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
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let rgba_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rgba video renderer pipeline layout"),
            bind_group_layouts: &[Some(&rgba_texture_bind_group_layout)],
            immediate_size: 0,
        });

        let yuv420p_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("yuv420p video renderer pipeline layout"),
                bind_group_layouts: &[Some(&yuv420p_texture_bind_group_layout)],
                immediate_size: 0,
            });

        let overlay_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("overlay renderer pipeline layout"),
                bind_group_layouts: &[Some(&rgba_texture_bind_group_layout)],
                immediate_size: 0,
            });

        let rgba_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rgba video renderer shader"),
            source: wgpu::ShaderSource::Wgsl(RGBA_VIDEO_SHADER.into()),
        });

        let yuv420p_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("yuv420p video renderer shader"),
            source: wgpu::ShaderSource::Wgsl(YUV420P_VIDEO_SHADER.into()),
        });

        let overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay renderer shader"),
            source: wgpu::ShaderSource::Wgsl(RGBA_VIDEO_SHADER.into()),
        });

        let video_rgba_pipeline = create_pipeline(
            &device,
            &rgba_pipeline_layout,
            &rgba_shader,
            config.format,
            Some(wgpu::BlendState::REPLACE),
            "rgba video renderer pipeline",
        );
        let video_yuv420p_pipeline = create_pipeline(
            &device,
            &yuv420p_pipeline_layout,
            &yuv420p_shader,
            config.format,
            Some(wgpu::BlendState::REPLACE),
            "yuv420p video renderer pipeline",
        );
        let overlay_pipeline = create_pipeline(
            &device,
            &overlay_pipeline_layout,
            &overlay_shader,
            config.format,
            Some(wgpu::BlendState::ALPHA_BLENDING),
            "overlay renderer pipeline",
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let video_texture_size = extent_from_size(frame_size.0.max(1), frame_size.1.max(1));
        let (video_texture, video_bind_group) = create_rgba_texture_and_bind_group(
            &device,
            &rgba_texture_bind_group_layout,
            &sampler,
            video_texture_size,
            "video texture",
            "video texture bind group",
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            rgba_texture_bind_group_layout,
            yuv420p_texture_bind_group_layout,
            sampler,
            video_rgba_pipeline,
            video_yuv420p_pipeline,
            overlay_pipeline,
            video_binding: UploadedVideoFrame::Rgba {
                bind_group: video_bind_group,
                texture: video_texture,
                texture_size: video_texture_size,
            },
            overlay_bind_group: None,
            overlay_texture: None,
            overlay_texture_size: None,
            video_frame_size: (frame_size.0.max(1), frame_size.1.max(1)),
            render_mode: RenderMode::Fit,
        })
    }

    pub fn set_render_mode(&mut self, render_mode: RenderMode) {
        self.render_mode = render_mode;
    }

    pub fn render_mode(&self) -> RenderMode {
        self.render_mode
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }

        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn video_display_rect(&self) -> DisplayRect {
        compute_display_rect(
            self.config.width,
            self.config.height,
            self.video_frame_size.0,
            self.video_frame_size.1,
            self.render_mode,
        )
    }

    pub fn upload_frame(&mut self, frame: &VideoFrameTexture) {
        match frame {
            VideoFrameTexture::Rgba(frame) => self.upload_rgba_video_frame(frame),
            VideoFrameTexture::Yuv420p(frame) => self.upload_yuv420p_video_frame(frame),
        }
    }

    fn upload_rgba_video_frame(&mut self, frame: &RgbaVideoFrame) {
        let size = extent_from_size(frame.width.max(1), frame.height.max(1));
        let needs_recreate = !matches!(
            &self.video_binding,
            UploadedVideoFrame::Rgba { texture_size, .. } if *texture_size == size
        );
        if needs_recreate {
            let (texture, bind_group) = create_rgba_texture_and_bind_group(
                &self.device,
                &self.rgba_texture_bind_group_layout,
                &self.sampler,
                size,
                "video texture",
                "video texture bind group",
            );
            self.video_binding = UploadedVideoFrame::Rgba {
                bind_group,
                texture,
                texture_size: size,
            };
        }

        self.video_frame_size = (frame.width.max(1), frame.height.max(1));
        let UploadedVideoFrame::Rgba { texture, .. } = &self.video_binding else {
            return;
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * 4),
                rows_per_image: Some(frame.height),
            },
            size,
        );
    }

    fn upload_yuv420p_video_frame(&mut self, frame: &Yuv420pVideoFrame) {
        let width = frame.width.max(1);
        let height = frame.height.max(1);
        let y_size = extent_from_size(width, height);
        let uv_width = width.div_ceil(2);
        let uv_height = height.div_ceil(2);
        let uv_size = extent_from_size(uv_width, uv_height);
        let expected_len =
            width as usize * height as usize + uv_width as usize * uv_height as usize * 2;
        if frame.bytes.len() < expected_len {
            return;
        }

        let needs_recreate = !matches!(
            &self.video_binding,
            UploadedVideoFrame::Yuv420p {
                y_size: current_y_size,
                uv_size: current_uv_size,
                ..
            } if *current_y_size == y_size && *current_uv_size == uv_size
        );
        if needs_recreate {
            let (y_texture, u_texture, v_texture, bind_group) =
                create_yuv420p_textures_and_bind_group(
                    &self.device,
                    &self.yuv420p_texture_bind_group_layout,
                    &self.sampler,
                    y_size,
                    uv_size,
                    "yuv420p video texture",
                    "yuv420p video texture bind group",
                );
            self.video_binding = UploadedVideoFrame::Yuv420p {
                bind_group,
                y_texture,
                u_texture,
                v_texture,
                y_size,
                uv_size,
            };
        }

        self.video_frame_size = (width, height);
        let UploadedVideoFrame::Yuv420p {
            y_texture,
            u_texture,
            v_texture,
            ..
        } = &self.video_binding
        else {
            return;
        };

        let y_plane_len = width as usize * height as usize;
        let uv_plane_len = uv_width as usize * uv_height as usize;
        let y_plane = &frame.bytes[..y_plane_len];
        let u_plane = &frame.bytes[y_plane_len..y_plane_len + uv_plane_len];
        let v_plane = &frame.bytes[y_plane_len + uv_plane_len..y_plane_len + uv_plane_len * 2];

        write_plane_texture(&self.queue, y_texture, y_plane, width, height);
        write_plane_texture(&self.queue, u_texture, u_plane, uv_width, uv_height);
        write_plane_texture(&self.queue, v_texture, v_plane, uv_width, uv_height);
    }

    pub fn upload_overlay(&mut self, overlay: &RgbaOverlayFrame) {
        let size = extent_from_size(overlay.width.max(1), overlay.height.max(1));
        if self.overlay_texture_size != Some(size) {
            let (texture, bind_group) = create_rgba_texture_and_bind_group(
                &self.device,
                &self.rgba_texture_bind_group_layout,
                &self.sampler,
                size,
                "overlay texture",
                "overlay texture bind group",
            );
            self.overlay_texture = Some(texture);
            self.overlay_bind_group = Some(bind_group);
            self.overlay_texture_size = Some(size);
        }

        if let Some(texture) = self.overlay_texture.as_ref() {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &overlay.bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(overlay.width * 4),
                    rows_per_image: Some(overlay.height),
                },
                size,
            );
        }
    }

    pub fn clear_overlay(&mut self) {
        self.overlay_bind_group = None;
        self.overlay_texture = None;
        self.overlay_texture_size = None;
    }

    pub fn render(&mut self) -> Result<()> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("surface texture acquisition failed with validation error");
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("video renderer command encoder"),
            });

        {
            let color_attachment = Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.02,
                        g: 0.02,
                        b: 0.03,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            });
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video renderer pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            let video_rect = self.video_display_rect();
            match &self.video_binding {
                UploadedVideoFrame::Rgba { bind_group, .. } => {
                    render_pass.set_pipeline(&self.video_rgba_pipeline);
                    render_pass.set_bind_group(0, bind_group, &[]);
                }
                UploadedVideoFrame::Yuv420p { bind_group, .. } => {
                    render_pass.set_pipeline(&self.video_yuv420p_pipeline);
                    render_pass.set_bind_group(0, bind_group, &[]);
                }
            }
            render_pass.set_viewport(
                video_rect.x as f32,
                video_rect.y as f32,
                video_rect.width.max(1) as f32,
                video_rect.height.max(1) as f32,
                0.0,
                1.0,
            );
            render_pass.draw(0..3, 0..1);

            if let (Some(bind_group), Some(size)) =
                (self.overlay_bind_group.as_ref(), self.overlay_texture_size)
            {
                render_pass.set_pipeline(&self.overlay_pipeline);
                render_pass.set_bind_group(0, bind_group, &[]);
                render_pass.set_viewport(
                    0.0,
                    0.0,
                    self.config.width as f32,
                    self.config.height as f32,
                    0.0,
                    1.0,
                );
                if size.width == self.config.width && size.height == self.config.height {
                    render_pass.draw(0..3, 0..1);
                }
            }
        }

        self.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    surface_format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn create_rgba_texture_and_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    size: wgpu::Extent3d,
    texture_label: &str,
    bind_group_label: &str,
) -> (wgpu::Texture, wgpu::BindGroup) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(texture_label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(bind_group_label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });

    (texture, bind_group)
}

fn create_yuv420p_textures_and_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    y_size: wgpu::Extent3d,
    uv_size: wgpu::Extent3d,
    texture_label: &str,
    bind_group_label: &str,
) -> (wgpu::Texture, wgpu::Texture, wgpu::Texture, wgpu::BindGroup) {
    let y_texture = create_plane_texture(device, y_size, &format!("{texture_label} y"));
    let u_texture = create_plane_texture(device, uv_size, &format!("{texture_label} u"));
    let v_texture = create_plane_texture(device, uv_size, &format!("{texture_label} v"));
    let y_view = y_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let u_view = u_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let v_view = v_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(bind_group_label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&y_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&u_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&v_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });

    (y_texture, u_texture, v_texture, bind_group)
}

fn create_plane_texture(device: &wgpu::Device, size: wgpu::Extent3d, label: &str) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_plane_texture(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    bytes: &[u8],
    width: u32,
    height: u32,
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width),
            rows_per_image: Some(height),
        },
        extent_from_size(width, height),
    );
}

fn extent_from_size(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

fn preferred_surface_format(formats: Vec<wgpu::TextureFormat>) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
        .or_else(|| formats.first().copied())
}

fn compute_display_rect(
    surface_width: u32,
    surface_height: u32,
    frame_width: u32,
    frame_height: u32,
    render_mode: RenderMode,
) -> DisplayRect {
    let surface_width = surface_width.max(1);
    let surface_height = surface_height.max(1);
    let frame_width = frame_width.max(1);
    let frame_height = frame_height.max(1);

    match render_mode {
        RenderMode::Stretch => DisplayRect {
            x: 0,
            y: 0,
            width: surface_width,
            height: surface_height,
        },
        RenderMode::Fit | RenderMode::Fill => {
            let scale_x = surface_width as f32 / frame_width as f32;
            let scale_y = surface_height as f32 / frame_height as f32;
            let scale = match render_mode {
                RenderMode::Fit => scale_x.min(scale_y),
                RenderMode::Fill => scale_x.max(scale_y),
                RenderMode::Stretch => unreachable!(),
            };
            let width = ((frame_width as f32 * scale).round() as u32).max(1);
            let height = ((frame_height as f32 * scale).round() as u32).max(1);
            let x = surface_width.saturating_sub(width) / 2;
            let y = surface_height.saturating_sub(height) / 2;

            DisplayRect {
                x,
                y,
                width: width.min(surface_width),
                height: height.min(surface_height),
            }
        }
    }
}

const RGBA_VIDEO_SHADER: &str = r#"
@group(0) @binding(0)
var video_texture: texture_2d<f32>;

@group(0) @binding(1)
var video_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 2.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
    );

    var output: VertexOutput;
    output.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    output.uv = uvs[vertex_index];
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(video_texture, video_sampler, input.uv);
}
"#;

const YUV420P_VIDEO_SHADER: &str = r#"
@group(0) @binding(0)
var y_texture: texture_2d<f32>;

@group(0) @binding(1)
var u_texture: texture_2d<f32>;

@group(0) @binding(2)
var v_texture: texture_2d<f32>;

@group(0) @binding(3)
var video_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

fn srgb_channel_to_linear(value: f32) -> f32 {
    if (value <= 0.04045) {
        return value / 12.92;
    }

    return pow((value + 0.055) / 1.055, 2.4);
}

fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        srgb_channel_to_linear(color.r),
        srgb_channel_to_linear(color.g),
        srgb_channel_to_linear(color.b),
    );
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 2.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
    );

    var output: VertexOutput;
    output.position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    output.uv = uvs[vertex_index];
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // Most SDR H.264/YUV420P desktop content arrives as limited-range Y'CbCr.
    let y = textureSample(y_texture, video_sampler, input.uv).r;
    let u = textureSample(u_texture, video_sampler, input.uv).r;
    let v = textureSample(v_texture, video_sampler, input.uv).r;

    let limited_y = max(y - (16.0 / 255.0), 0.0) * (255.0 / 219.0);
    let cb = (u - (128.0 / 255.0)) * (255.0 / 224.0);
    let cr = (v - (128.0 / 255.0)) * (255.0 / 224.0);

    // Rec.709 coefficients produce gamma-compressed RGB; convert to linear
    // before writing into the sRGB swapchain.
    let gamma_rgb = clamp(
        vec3<f32>(
            limited_y + 1.5748 * cr,
            limited_y - 0.1873 * cb - 0.4681 * cr,
            limited_y + 1.8556 * cb,
        ),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
    let linear_rgb = srgb_to_linear(gamma_rgb);
    return vec4<f32>(linear_rgb, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::{DisplayRect, RenderMode, compute_display_rect};

    #[test]
    fn fit_preserves_aspect_ratio_inside_surface() {
        let rect = compute_display_rect(1280, 720, 960, 432, RenderMode::Fit);
        assert_eq!(
            rect,
            DisplayRect {
                x: 0,
                y: 72,
                width: 1280,
                height: 576,
            }
        );
    }

    #[test]
    fn stretch_uses_full_surface() {
        let rect = compute_display_rect(1280, 720, 960, 432, RenderMode::Stretch);
        assert_eq!(
            rect,
            DisplayRect {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
            }
        );
    }
}
