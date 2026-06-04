pub(crate) enum UploadedVideoFrame {
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

pub(crate) fn create_rgba_texture_and_bind_group(
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

pub(crate) fn create_yuv420p_textures_and_bind_group(
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

pub(crate) fn write_plane_texture(
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

pub(crate) fn extent_from_size(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}
