pub(crate) const RGBA_VIDEO_SHADER: &str = r#"
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

pub(crate) const YUV420P_VIDEO_SHADER: &str = r#"
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
