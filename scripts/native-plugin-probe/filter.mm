#include "filter.h"

#import <Metal/Metal.h>

namespace {

// This shader exercises two imported planes and a GPU-only intermediate target.
NSString *const kernel = @R"METAL(
    #include <metal_stdlib>
    using namespace metal;
    kernel void filter_main(texture2d<float, access::read> y [[texture(0)]],
                            texture2d<float, access::read> uv [[texture(1)]],
                            texture2d<float, access::write> dst [[texture(2)]],
                            uint2 p [[thread_position_in_grid]]) {
        if (p.x >= dst.get_width() || p.y >= dst.get_height()) return;
        float luma = y.read(p).r * 0.85;
        float2 chroma = uv.read(p / 2).rg - 0.5;
        dst.write(float4(luma + 1.5748 * chroma.y,
                         luma - 0.1873 * chroma.x - 0.4681 * chroma.y,
                         luma + 1.8556 * chroma.x, 1.0), p);
    }
)METAL";

bool is_metal(const VesperProbeGpuContext *context) noexcept {
    return context && context->struct_size >= sizeof(VesperProbeGpuContext)
        && context->backend == VESPER_PROBE_GPU_METAL && context->device;
}

} // namespace

extern "C" void *vesper_probe_filter_open(const VesperProbeGpuContext *context) noexcept {
    if (!is_metal(context)) return nullptr;
    id<MTLDevice> device = (__bridge id<MTLDevice>)context->device;
    NSError *error = nil;
    auto library = [device newLibraryWithSource:kernel options:nil error:&error];
    if (!library) {
        NSLog(@"Metal probe shader compile failed: %@", error);
        return nullptr;
    }
    auto function = [library newFunctionWithName:@"filter_main"];
    return (__bridge_retained void *)[device newComputePipelineStateWithFunction:function error:&error];
}

extern "C" uint32_t vesper_probe_filter_encode(void *filter, const VesperProbeGpuContext *context,
                                               const VesperProbeVideoFormat *format, void *y,
                                               void *uv, void *out) noexcept {
    if (!filter || !is_metal(context) || !context->command_buffer || !format
        || format->struct_size < sizeof(VesperProbeVideoFormat) || !y || !uv || !out
        || (format->pixel_format != VESPER_PROBE_VIDEO_NV12
            && format->pixel_format != VESPER_PROBE_VIDEO_P010)) {
        return VESPER_PROBE_INVALID;
    }
    // The host supplies all resources and owns command-buffer submission.
    id<MTLTexture> target = (__bridge id<MTLTexture>)out;
    if (target.width != format->width || target.height != format->height) {
        return VESPER_PROBE_INVALID;
    }
    id<MTLCommandBuffer> command = (__bridge id<MTLCommandBuffer>)context->command_buffer;
    id<MTLComputeCommandEncoder> encoder = [command computeCommandEncoder];
    if (!encoder) return VESPER_PROBE_INTERNAL;
    [encoder setComputePipelineState:(__bridge id<MTLComputePipelineState>)filter];
    [encoder setTexture:(__bridge id<MTLTexture>)y atIndex:0];
    [encoder setTexture:(__bridge id<MTLTexture>)uv atIndex:1];
    [encoder setTexture:target atIndex:2];
    [encoder dispatchThreads:MTLSizeMake(format->width, format->height, 1)
         threadsPerThreadgroup:MTLSizeMake(16, 8, 1)];
    [encoder endEncoding];
    return VESPER_PROBE_OK;
}

extern "C" void vesper_probe_filter_close(void *filter) noexcept {
    (void)CFBridgingRelease(filter);
}
