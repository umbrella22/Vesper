use std::sync::Arc;

use anyhow::{Context, Result};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::pipeline::create_pipeline;
use crate::shaders::{RGBA_VIDEO_SHADER, YUV420P_VIDEO_SHADER};
use crate::types::{
    DisplayRect, RenderFrameOutcome, RenderMode, RgbaOverlayFrame, RgbaVideoFrame,
    VideoFrameTexture, Yuv420pVideoFrame,
};
use crate::upload::{
    UploadedVideoFrame, create_rgba_texture_and_bind_group, create_yuv420p_textures_and_bind_group,
    extent_from_size, write_plane_texture,
};
use crate::validation::{
    TextureInputLimits, validate_rgba_upload, validate_texture_dimensions, validate_yuv420p_upload,
};
use crate::viewport::compute_display_rect;
use crate::window::{preferred_backend_label, preferred_backends, preferred_surface_format};

pub struct VideoRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    texture_input_limits: TextureInputLimits,
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
    video_viewport: Option<DisplayRect>,
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
                apply_limit_buckets: false,
            })
            .await
            .with_context(|| {
                format!(
                    "failed to request a {} wgpu adapter",
                    preferred_backend_label()
                )
            })?;

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
        let initial_width = initial_size.width.max(1);
        let initial_height = initial_size.height.max(1);
        let device_limits = device.limits();
        let texture_input_limits = TextureInputLimits::for_device(
            device_limits.max_texture_dimension_2d,
            device_limits.max_buffer_size,
        );
        validate_texture_dimensions(
            "initial render surface",
            initial_width,
            initial_height,
            texture_input_limits.max_dimension_2d,
        )?;
        let mut config = surface
            .get_default_config(&adapter, initial_width, initial_height)
            .context("surface does not support default configuration")?;
        let capabilities = surface.get_capabilities(&adapter);
        config.format = preferred_surface_format(&capabilities.formats).unwrap_or(config.format);
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

        validate_texture_dimensions(
            "initial video frame",
            frame_size.0,
            frame_size.1,
            texture_input_limits.max_dimension_2d,
        )?;
        let video_texture_size = extent_from_size(1, 1);
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
            texture_input_limits,
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
            video_frame_size: frame_size,
            render_mode: RenderMode::Fit,
            video_viewport: None,
        })
    }

    pub fn set_render_mode(&mut self, render_mode: RenderMode) {
        self.render_mode = render_mode;
    }

    pub fn set_video_viewport(&mut self, viewport: Option<DisplayRect>) {
        self.video_viewport = viewport;
    }

    pub fn render_mode(&self) -> RenderMode {
        self.render_mode
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) -> Result<()> {
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        validate_texture_dimensions(
            "render surface",
            size.width,
            size.height,
            self.texture_input_limits.max_dimension_2d,
        )?;

        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        Ok(())
    }

    pub fn video_display_rect(&self) -> DisplayRect {
        let viewport = self.video_viewport.unwrap_or(DisplayRect {
            x: 0,
            y: 0,
            width: self.config.width,
            height: self.config.height,
        });
        let mut rect = compute_display_rect(
            viewport.width,
            viewport.height,
            self.video_frame_size.0,
            self.video_frame_size.1,
            self.render_mode,
        );
        rect.x = rect.x.saturating_add(viewport.x);
        rect.y = rect.y.saturating_add(viewport.y);
        rect
    }

    pub fn upload_frame(&mut self, frame: &VideoFrameTexture) -> Result<()> {
        match frame {
            VideoFrameTexture::Rgba(frame) => self.upload_rgba_video_frame(frame),
            VideoFrameTexture::Yuv420p(frame) => self.upload_yuv420p_video_frame(frame),
        }
    }

    fn upload_rgba_video_frame(&mut self, frame: &RgbaVideoFrame) -> Result<()> {
        let layout = validate_rgba_upload(
            "RGBA video frame",
            frame.width,
            frame.height,
            frame.bytes.len(),
            self.texture_input_limits,
        )?;
        let size = extent_from_size(layout.width, layout.height);
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

        let UploadedVideoFrame::Rgba { texture, .. } = &self.video_binding else {
            anyhow::bail!("renderer RGBA binding was not installed before upload");
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
                bytes_per_row: Some(layout.bytes_per_row),
                rows_per_image: Some(layout.height),
            },
            size,
        );
        self.video_frame_size = (layout.width, layout.height);
        Ok(())
    }

    fn upload_yuv420p_video_frame(&mut self, frame: &Yuv420pVideoFrame) -> Result<()> {
        let layout = validate_yuv420p_upload(
            "YUV420p video frame",
            frame.width,
            frame.height,
            frame.bytes.len(),
            self.texture_input_limits,
        )?;
        let y_size = extent_from_size(layout.width, layout.height);
        let uv_size = extent_from_size(layout.uv_width, layout.uv_height);

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

        let UploadedVideoFrame::Yuv420p {
            y_texture,
            u_texture,
            v_texture,
            ..
        } = &self.video_binding
        else {
            anyhow::bail!("renderer YUV420p binding was not installed before upload");
        };

        let y_plane = &frame.bytes[..layout.y_plane_len];
        let u_plane = &frame.bytes[layout.y_plane_len..layout.y_plane_len + layout.uv_plane_len];
        let v_plane = &frame.bytes[layout.y_plane_len + layout.uv_plane_len..layout.expected_len];

        write_plane_texture(&self.queue, y_texture, y_plane, layout.width, layout.height);
        write_plane_texture(
            &self.queue,
            u_texture,
            u_plane,
            layout.uv_width,
            layout.uv_height,
        );
        write_plane_texture(
            &self.queue,
            v_texture,
            v_plane,
            layout.uv_width,
            layout.uv_height,
        );
        self.video_frame_size = (layout.width, layout.height);
        Ok(())
    }

    pub fn upload_overlay(&mut self, overlay: &RgbaOverlayFrame) -> Result<()> {
        let layout = validate_rgba_upload(
            "RGBA overlay",
            overlay.width,
            overlay.height,
            overlay.bytes.len(),
            self.texture_input_limits,
        )?;
        let size = extent_from_size(layout.width, layout.height);
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
                    bytes_per_row: Some(layout.bytes_per_row),
                    rows_per_image: Some(layout.height),
                },
                size,
            );
            Ok(())
        } else {
            anyhow::bail!("renderer overlay binding was not installed before upload");
        }
    }

    pub fn clear_overlay(&mut self) {
        self.overlay_bind_group = None;
        self.overlay_texture = None;
        self.overlay_texture_size = None;
    }

    pub fn render(&mut self) -> Result<()> {
        self.render_with_outcome().map(|_| ())
    }

    pub fn render_with_outcome(&mut self) -> Result<RenderFrameOutcome> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(RenderFrameOutcome::Timeout);
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(RenderFrameOutcome::Occluded);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(RenderFrameOutcome::SurfaceReconfigured);
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
                if size.width > 0 && size.height > 0 {
                    render_pass.draw(0..3, 0..1);
                }
            }
        }

        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(RenderFrameOutcome::Presented)
    }
}
