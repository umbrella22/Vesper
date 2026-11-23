#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <atomic>
#include <cstring>
#include "metal_host.h"

extern "C" const VesperVideoFilterVtable *vesper_reference_metal_filter_vtable() noexcept;

@interface VesperMetalFilterSlot : NSObject {
@public
    std::atomic_bool occupied;
}
@property(nonatomic, strong) id<MTLTexture> texture;
@end

@implementation VesperMetalFilterSlot
- (instancetype)init {
    if ((self = [super init])) occupied.store(false, std::memory_order_relaxed);
    return self;
}
@end

// The command handler retains this object independently of the presenter. If
// the command is abandoned before submission, block destruction retires it.
@interface VesperMetalFilterUse : NSObject
@property(nonatomic, strong) VesperMetalFilterSlot *slot;
@property(nonatomic, strong) id pixelBuffer;
@property(nonatomic, strong) id lumaImport;
@property(nonatomic, strong) id chromaImport;
- (void)retire;
@end

@implementation VesperMetalFilterUse
- (void)retire {
    if (self.slot != nil) {
        self.slot->occupied.store(false, std::memory_order_release);
        self.slot = nil;
    }
    self.pixelBuffer = nil;
    self.lumaImport = nil;
    self.chromaImport = nil;
}
- (void)dealloc { [self retire]; }
@end

@interface VesperMetalFilterHost : NSObject
@property(nonatomic, strong) id<MTLDevice> device;
@property(nonatomic, strong) id<MTLCommandQueue> queue;
@property(nonatomic, strong) id cacheOwner;
@property(nonatomic, strong) NSData *parameters;
@property(nonatomic, strong) NSArray<VesperMetalFilterSlot *> *slots;
@property(nonatomic, assign) VesperSession *session;
@property(nonatomic, assign) VesperVideoFormat inputFormat;
@property(nonatomic, assign) VesperVideoFormat outputFormat;
@end

@implementation VesperMetalFilterHost
- (void)dealloc {
    if (self.session) vesper_reference_metal_filter_vtable()->destroy(self.session);
}
@end

void *vesper_reference_metal_host_create(const void *device, const void *queue,
                                         CVMetalTextureCacheRef supplied_cache, const uint8_t *parameters, size_t parameters_len) {
    if (!device || !queue || !parameters || !parameters_len || parameters_len > 65536) return nullptr;
    auto *host = [VesperMetalFilterHost new];
    host.device = (__bridge id<MTLDevice>)device;
    host.queue = (__bridge id<MTLCommandQueue>)queue;
    if (supplied_cache) {
        host.cacheOwner = (__bridge id)supplied_cache;
    } else {
        CVMetalTextureCacheRef cache = nullptr;
        if (CVMetalTextureCacheCreate(kCFAllocatorDefault, nullptr, host.device, nullptr, &cache) != kCVReturnSuccess) {
            return nullptr;
        }
        host.cacheOwner = CFBridgingRelease(cache);
    }
    host.parameters = [NSData dataWithBytes:parameters length:parameters_len];
    host.slots = @[[VesperMetalFilterSlot new], [VesperMetalFilterSlot new], [VesperMetalFilterSlot new]];
    return (__bridge_retained void *)host;
}

void vesper_reference_metal_host_destroy(void *host) {
    (void)CFBridgingRelease(host);
}

static uint8_t color_code(CVPixelBufferRef pixel_buffer, CFStringRef key,
                          int (*code_point)(CFStringRef), uint8_t fallback) {
    id attachment = nil;
    if (@available(macOS 12.0, *)) {
        attachment = CFBridgingRelease(CVBufferCopyAttachment(pixel_buffer, key, nullptr));
    } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        attachment = (__bridge id)CVBufferGetAttachment(pixel_buffer, key, nullptr);
#pragma clang diagnostic pop
    }
    if (attachment == nil) return fallback;
    if (![attachment isKindOfClass:NSString.class]) return 2;
    const int value = code_point((__bridge CFStringRef)attachment);
    return value >= 0 && value <= UINT8_MAX ? static_cast<uint8_t>(value) : 2;
}

static id<MTLTexture> import_plane(CVMetalTextureCacheRef cache, CVPixelBufferRef pixel_buffer,
                                   MTLPixelFormat format, size_t index, id __strong *owner) {
    CVMetalTextureRef imported = nullptr;
    const CVReturn status = CVMetalTextureCacheCreateTextureFromImage(kCFAllocatorDefault, cache,
        pixel_buffer, nullptr, format, CVPixelBufferGetWidthOfPlane(pixel_buffer, index),
        CVPixelBufferGetHeightOfPlane(pixel_buffer, index), index, &imported);
    if (status != kCVReturnSuccess || imported == nullptr) return nil;
    id<MTLTexture> texture = CVMetalTextureGetTexture(imported);
    *owner = CFBridgingRelease(imported);
    return texture;
}

VesperStatus vesper_reference_metal_host_encode(void *handle, const void *command,
                                                CVPixelBufferRef pixel_buffer,
                                                const void *luma, const void *chroma,
                                                const void **out_texture) {
    if (!out_texture) return VESPER_ERR_INVALID_ARG;
    *out_texture = nullptr;
    if (!handle || !command || !pixel_buffer || CVPixelBufferGetPlaneCount(pixel_buffer) != 2) {
        return VESPER_ERR_INVALID_ARG;
    }
    const OSType pixel_format = CVPixelBufferGetPixelFormatType(pixel_buffer);
    const bool p010 = pixel_format == kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange ||
                      pixel_format == kCVPixelFormatType_420YpCbCr10BiPlanarFullRange;
    if (!p010 && pixel_format != kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange &&
                 pixel_format != kCVPixelFormatType_420YpCbCr8BiPlanarFullRange) return VESPER_ERR_UNSUPPORTED;
    VesperMetalFilterHost *host = (__bridge VesperMetalFilterHost *)handle;
    id<MTLCommandBuffer> buffer = (__bridge id<MTLCommandBuffer>)command;
    CVMetalTextureCacheRef cache = (__bridge CVMetalTextureCacheRef)host.cacheOwner;
    id<MTLTexture> y = (__bridge id<MTLTexture>)luma;
    id<MTLTexture> uv = (__bridge id<MTLTexture>)chroma;
    id y_ref = nil, uv_ref = nil;
    if (y == nil) y = import_plane(cache, pixel_buffer, p010 ? MTLPixelFormatR16Unorm : MTLPixelFormatR8Unorm, 0, &y_ref);
    if (uv == nil) uv = import_plane(cache, pixel_buffer, p010 ? MTLPixelFormatRG16Unorm : MTLPixelFormatRG8Unorm, 1, &uv_ref);
    if (y == nil || uv == nil) return VESPER_ERR_UNSUPPORTED;

    VesperVideoFormat next = {};
    next.struct_size = sizeof(next);
    next.width = next.coded_width = next.crop_w = (uint32_t)y.width;
    next.height = next.coded_height = next.crop_h = (uint32_t)y.height;
    next.pixel_format = p010 ? VESPER_PIXEL_P010 : VESPER_PIXEL_NV12;
    next.color.bit_depth = p010 ? 10 : 8;
    next.color.primaries = color_code(pixel_buffer, kCVImageBufferColorPrimariesKey,
        CVColorPrimariesGetIntegerCodePointForString, p010 ? 9 : 1);
    next.color.matrix = color_code(pixel_buffer, kCVImageBufferYCbCrMatrixKey,
        CVYCbCrMatrixGetIntegerCodePointForString, p010 ? 9 : 1);
    next.color.transfer = color_code(pixel_buffer, kCVImageBufferTransferFunctionKey,
        CVTransferFunctionGetIntegerCodePointForString, 1);
    next.color.hdr_kind = next.color.transfer == 16 ? VESPER_HDR_HDR10 : VESPER_HDR_SDR;
    next.color.range = (pixel_format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange ||
                        pixel_format == kCVPixelFormatType_420YpCbCr10BiPlanarFullRange) ? 2 : 1;

    VesperGpuContext gpu = {};
    gpu.struct_size = sizeof(gpu);
    gpu.backend = VESPER_GPU_METAL;
    gpu.data.metal = {(__bridge void *)host.device, (__bridge void *)host.queue,
                      (__bridge void *)buffer, (void *)cache};
    const auto *table = vesper_reference_metal_filter_vtable();
    const VesperVideoFormat previous = host.inputFormat;
    if (host.session == nullptr || previous.width != next.width || previous.height != next.height ||
        previous.pixel_format != next.pixel_format || std::memcmp(&previous.color, &next.color, sizeof(next.color)) != 0) {
        next.format_id = previous.format_id + 1;
        VesperVideoFormat output = {};
        output.struct_size = sizeof(output);
        VesperSession *session = nullptr;
        const VesperBytes params = {(const uint8_t *)host.parameters.bytes, host.parameters.length};
        const VesperStatus status = table->open(nullptr, &gpu, &next, params, &output, &session);
        if (status != VESPER_OK || session == nullptr) return status == VESPER_OK ? VESPER_ERR_INTERNAL : status;
        if (host.session != nullptr) table->destroy(host.session);
        host.session = session;
        host.inputFormat = next;
        host.outputFormat = output;
    }
    for (VesperMetalFilterSlot *slot in host.slots) {
        bool vacant = false;
        if (!slot->occupied.compare_exchange_strong(vacant, true, std::memory_order_acquire, std::memory_order_relaxed)) continue;
        auto *lease = [VesperMetalFilterUse new];
        lease.slot = slot;
        lease.pixelBuffer = (__bridge id)pixel_buffer;
        lease.lumaImport = y_ref;
        lease.chromaImport = uv_ref;
        if (slot.texture.width != next.width || slot.texture.height != next.height) {
            auto *descriptor = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatRGBA16Float
                width:next.width height:next.height mipmapped:NO];
            descriptor.storageMode = MTLStorageModePrivate;
            descriptor.usage = MTLTextureUsageShaderRead | MTLTextureUsageShaderWrite;
            slot.texture = [host.device newTextureWithDescriptor:descriptor];
        }
        VesperVideoFrame input = {};
        input.struct_size = sizeof(input);
        input.format_id = host.inputFormat.format_id;
        input.pts_us = input.duration_us = INT64_MIN;
        input.handle_kind = VESPER_HANDLE_MTL_TEXTURE;
        input.plane_count = 2;
        input.handles[0] = (uintptr_t)(__bridge void *)y;
        input.handles[1] = (uintptr_t)(__bridge void *)uv;
        VesperVideoFrame output = {};
        output.struct_size = sizeof(output);
        output.format_id = host.outputFormat.format_id;
        output.handle_kind = VESPER_HANDLE_MTL_TEXTURE;
        output.plane_count = 1;
        output.handles[0] = (uintptr_t)(__bridge void *)slot.texture;
        const VesperStatus status = table->process(host.session, &gpu, &input, &output);
        if (status != VESPER_OK) return status;
        [buffer addCompletedHandler:^(__unused id<MTLCommandBuffer> completed) { [lease retire]; }];
        *out_texture = (__bridge const void *)slot.texture;
        return VESPER_OK;
    }
    return VESPER_TRY_AGAIN;
}
