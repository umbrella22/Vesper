#include "vesper_plugin.h"

#import <Foundation/Foundation.h>
#import <Metal/Metal.h>

#include <cmath>
#include <cstring>
#include <new>


namespace {

struct FilterParams {
    uint32_t width, height, bit_depth, range, primaries, matrix, transfer;
    float gain;
};
static_assert(sizeof(FilterParams) == 32);

struct Session {
    id<MTLComputePipelineState> pipeline;
    FilterParams params;
    VesperVideoFormat input;
    VesperVideoFormat output;
};

VesperBytes bytes(const char *data) noexcept {
    return {reinterpret_cast<const uint8_t *>(data), std::strlen(data)};
}

bool valid_context(const VesperGpuContext *gpu) noexcept {
    return gpu && gpu->struct_size >= sizeof(VesperGpuContext)
        && gpu->backend == VESPER_GPU_METAL && gpu->data.metal.device;
}

VesperStatus capabilities(void *, VesperBytes *out) noexcept {
    if (!out) return VESPER_ERR_INVALID_ARG;
    *out = bytes(R"({"gpu":"metal","input":["NV12","P010"],"output":"RGBA16F","requiresPooledOutput":true})");
    return VESPER_OK;
}

VesperStatus open(void *, const VesperGpuContext *gpu, const VesperVideoFormat *input,
                  VesperBytes params, VesperVideoFormat *output, VesperSession **session) noexcept {
    if (!session) return VESPER_ERR_INVALID_ARG;
    *session = nullptr;
    if (!valid_context(gpu) || !input || input->struct_size < sizeof(VesperVideoFormat)
        || !output || output->struct_size < sizeof(VesperVideoFormat) || !params.data
        || params.len == 0 || params.len > 16384 || !input->format_id || !input->width || !input->height) {
        return VESPER_ERR_INVALID_ARG;
    }
    const auto supported_chromaticities = [](uint8_t code) {
        return code == 1 || code == 5 || code == 6 || code == 9;
    };
    if ((input->pixel_format != VESPER_PIXEL_NV12 && input->pixel_format != VESPER_PIXEL_P010)
        || !supported_chromaticities(input->color.matrix)
        || !supported_chromaticities(input->color.primaries)
        || (input->color.transfer != 1 && input->color.transfer != 6 && input->color.transfer != 16)
        || input->color.hdr_kind > VESPER_HDR_HDR10) return VESPER_ERR_UNSUPPORTED;

    @autoreleasepool {
        NSData *data = [NSData dataWithBytes:params.data length:params.len];
        id json = [NSJSONSerialization JSONObjectWithData:data options:0 error:nullptr];
        if (![json isKindOfClass:NSDictionary.class]) return VESPER_ERR_INVALID_ARG;
        id path = json[@"metalLibrary"];
        if (![path isKindOfClass:NSString.class] || ![path isAbsolutePath]) return VESPER_ERR_INVALID_ARG;
        id<MTLDevice> device = (__bridge id<MTLDevice>)gpu->data.metal.device;
        id<MTLLibrary> library = [device newLibraryWithURL:[NSURL fileURLWithPath:path] error:nullptr];
        if (!library) return VESPER_ERR_UNSUPPORTED;
        id<MTLFunction> function = [library newFunctionWithName:@"colorFilter"];
        if (!function) return VESPER_ERR_UNSUPPORTED;
        id<MTLComputePipelineState> pipeline = [device newComputePipelineStateWithFunction:function error:nullptr];
        if (!pipeline || pipeline.maxTotalThreadsPerThreadgroup < 128) return VESPER_ERR_UNSUPPORTED;
        auto *state = new (std::nothrow) Session{
            pipeline,
            {input->width, input->height, input->pixel_format == VESPER_PIXEL_P010 ? 10u : 8u,
             input->color.range, input->color.primaries, input->color.matrix, input->color.transfer, 1.0f},
            *input, *input,
        };
        if (!state) return VESPER_ERR_EXHAUSTED;
        state->output.struct_size = sizeof(VesperVideoFormat);
        state->output.pixel_format = VESPER_PIXEL_RGBA16F;
        state->output.color = {1, 8, 0, 2, 16, input->color.hdr_kind, {0, 0}};
        // Mastering metadata describes the compressed source's encoding gamut.
        state->output.has_hdr_static = 0;
        state->output.hdr_static = {};
        *output = state->output;
        *session = state;
        return VESPER_OK;
    }
}

VesperStatus set_param(VesperSession *raw, VesperBytes key, VesperBytes value) noexcept {
    if (!raw || !key.data || !value.data || value.len == 0 || value.len > 32) return VESPER_ERR_INVALID_ARG;
    if (key.len != 4 || std::memcmp(key.data, "gain", 4) != 0) return VESPER_ERR_UNSUPPORTED;
    @autoreleasepool {
        NSString *text = [[NSString alloc] initWithBytes:value.data length:value.len encoding:NSUTF8StringEncoding];
        if (!text) return VESPER_ERR_INVALID_ARG;
        NSScanner *scanner = [NSScanner scannerWithString:text];
        float gain;
        if (![scanner scanFloat:&gain] || !scanner.isAtEnd || !std::isfinite(gain) || gain < 0.0f || gain > 4.0f) {
            return VESPER_ERR_INVALID_ARG;
        }
        static_cast<Session *>(raw)->params.gain = gain;
    }
    return VESPER_OK;
}

VesperStatus process(VesperSession *raw, const VesperGpuContext *gpu,
                     const VesperVideoFrame *input, VesperVideoFrame *output) noexcept {
    if (!raw || !valid_context(gpu) || !gpu->data.metal.command_buffer) return VESPER_ERR_INVALID_ARG;
    if (!input) return VESPER_EOF;
    if (!output || input->struct_size < sizeof(VesperVideoFrame) || output->struct_size < sizeof(VesperVideoFrame)
        || input->handle_kind != VESPER_HANDLE_MTL_TEXTURE || input->plane_count != 2
        || output->handle_kind != VESPER_HANDLE_MTL_TEXTURE || output->plane_count != 1
        || !input->handles[0] || !input->handles[1] || !output->handles[0]) return VESPER_ERR_INVALID_ARG;
    auto &state = *static_cast<Session *>(raw);
    if (input->format_id != state.input.format_id) return VESPER_FORMAT_CHANGED;
    if (output->format_id != state.output.format_id) return VESPER_ERR_INVALID_ARG;
    auto command = (__bridge id<MTLCommandBuffer>)reinterpret_cast<void *>(gpu->data.metal.command_buffer);
    auto y = (__bridge id<MTLTexture>)reinterpret_cast<void *>(input->handles[0]);
    auto uv = (__bridge id<MTLTexture>)reinterpret_cast<void *>(input->handles[1]);
    auto target = (__bridge id<MTLTexture>)reinterpret_cast<void *>(output->handles[0]);
    const bool p010 = state.params.bit_depth == 10;
    if (y.device != state.pipeline.device || uv.device != y.device || target.device != y.device
        || y.pixelFormat != (p010 ? MTLPixelFormatR16Unorm : MTLPixelFormatR8Unorm)
        || uv.pixelFormat != (p010 ? MTLPixelFormatRG16Unorm : MTLPixelFormatRG8Unorm)
        || target.pixelFormat != MTLPixelFormatRGBA16Float || target == y || target == uv
        || y.width < state.params.width || y.height < state.params.height
        || uv.width < (state.params.width + 1ull) / 2 || uv.height < (state.params.height + 1ull) / 2
        || target.width != state.params.width || target.height != state.params.height) return VESPER_ERR_INVALID_ARG;

    id<MTLComputeCommandEncoder> encoder = [command computeCommandEncoder];
    if (!encoder) return VESPER_ERR_GPU_LOST;
    [encoder setComputePipelineState:state.pipeline];
    [encoder setTexture:y atIndex:0];
    [encoder setTexture:uv atIndex:1];
    [encoder setTexture:target atIndex:2];
    [encoder setBytes:&state.params length:sizeof(state.params) atIndex:0];
    [encoder dispatchThreads:MTLSizeMake(state.params.width, state.params.height, 1)
         threadsPerThreadgroup:MTLSizeMake(16, 8, 1)];
    [encoder endEncoding];
    output->pts_us = input->pts_us;
    output->duration_us = input->duration_us;
    return VESPER_OK;
}

VesperStatus format(VesperSession *raw, VesperVideoFormat *out) noexcept {
    if (!raw || !out || out->struct_size < sizeof(VesperVideoFormat)) return VESPER_ERR_INVALID_ARG;
    *out = static_cast<const Session *>(raw)->output;
    return VESPER_OK;
}
VesperStatus flush(VesperSession *raw) noexcept { return raw ? VESPER_OK : VESPER_ERR_INVALID_ARG; }
void destroy(VesperSession *raw) noexcept { delete static_cast<Session *>(raw); }

const VesperVideoFilterVtable table = {
    sizeof(VesperVideoFilterVtable), capabilities, open, set_param, process, format, flush, destroy, nullptr,
};

#if !defined(VESPER_REFERENCE_EMBEDDED)
VesperStatus interface_at(void *, uint32_t index, VesperInterfaceId *out) noexcept {
    if (index || !out) return VESPER_ERR_INVALID_ARG;
    *out = VESPER_VIDEO_FILTER_ID;
    return VESPER_OK;
}
VesperStatus query(void *, const VesperInterfaceId *id, const void **out) noexcept {
    if (!id || !out) return VESPER_ERR_INVALID_ARG;
    *out = nullptr;
    if (std::memcmp(id->bytes, VESPER_VIDEO_FILTER_ID.bytes, sizeof(id->bytes))) return VESPER_ERR_UNSUPPORTED;
    *out = &table;
    return VESPER_OK;
}
const VesperPluginRoot root = {
    sizeof(VesperPluginRoot), VESPER_PLUGIN_ABI_MAJOR, VESPER_PLUGIN_ABI_MINOR, nullptr,
    bytes("io.github.umbrella22.vesper-color-filter"), bytes("Vesper Slang Color Filter"), 1, interface_at, query, nullptr,
};
#endif

} // namespace

#if defined(VESPER_REFERENCE_EMBEDDED)
extern "C" const VesperVideoFilterVtable *vesper_reference_metal_filter_vtable() noexcept { return &table; }
#else
const VesperPluginRoot *vesper_native_plugin_entry() noexcept { return &root; }
#endif
