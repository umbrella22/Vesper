#include "video-color.slang"

struct FilterParams
{
    uint width;
    uint height;
    uint bitDepth;
    uint range;
    uint primaries;
    uint matrixCode;
    uint transfer;
    float gain;
};

#ifdef VESPER_VULKAN_COMBINED_420
[[vk::binding(0, 0)]] Sampler2D<float4> ycbcr;
#else
[[vk::binding(0, 0)]] Texture2D<float> luma : register(t0);
[[vk::binding(1, 0)]] Texture2D<float2> chroma : register(t1);
#endif
#ifdef VESPER_D3D11_SDR_TARGET
[format("rgba8")] RWTexture2D<float4> target : register(u0);
#else
[[vk::binding(2, 0)]] [format("rgba16f")] RWTexture2D<float4> target : register(u0);
#endif
[[vk::binding(3, 0)]] ConstantBuffer<FilterParams> filter : register(b0);

[shader("compute")]
[numthreads(16, 8, 1)]
void colorFilter(uint3 thread : SV_DispatchThreadID)
{
    if (thread.x >= filter.width || thread.y >= filter.height) {
        return;
    }
    uint2 px = thread.xy;
#ifdef VESPER_VULKAN_COMBINED_420
    float2 texCoord = (float2(px) + 0.5) / float2(filter.width, filter.height);
    // RGB_IDENTITY preserves the normalized YCbCr components as (Cr, Y, Cb).
    float3 sample = ycbcr.SampleLevel(texCoord, 0).rgb;
    float codeScale = filter.bitDepth == 10 ? 1023.0 : 255.0;
    float y = sample.g * codeScale;
    float2 uv = sample.br * codeScale;
#else
    float codeScale = filter.bitDepth == 10 ? 65535.0 / 64.0 : 255.0;
    float y = luma.Load(int3(px, 0)) * codeScale;
    float2 uv = chroma.Load(int3(px / 2, 0)) * codeScale;
#endif
    float3 signal = yuvCodesToNonlinearRgb(y, uv, filter.bitDepth, filter.range, filter.matrixCode);
    float3 linear = toLinearBt709(decodeTransfer(signal, filter.transfer), filter.primaries);
#ifdef VESPER_D3D11_SDR_TARGET
    // D3D11 passes encoded SDR RGB to its swapchain video processor.
    float3 sdr = saturate(linear * filter.gain);
    float3 srgb = select(sdr <= 0.0031308, 12.92 * sdr, 1.055 * pow(sdr, 1.0 / 2.4) - 0.055);
    target[px] = float4(srgb, 1.0);
#else
    target[px] = float4(linear * filter.gain, 1.0);
#endif
}
