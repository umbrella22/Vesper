#include "filter.h"

#import <AVFoundation/AVFoundation.h>
#import <CoreVideo/CoreVideo.h>
#import <Metal/Metal.h>

#include <array>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <memory>
#include <mutex>
#include <new>

namespace {

enum class SlotState { free, submitted, retired };

struct Slot {
    std::atomic<SlotState> state{SlotState::free};
    id<MTLTexture> target = nil;
};

struct Pool {
    std::array<Slot, 3> slots;
    uint32_t submitted = 0;
    std::atomic<uint32_t> released{0};
    std::atomic<uint32_t> failed{0};
    std::mutex completion_lock;
    std::condition_variable completed;
};

struct MetalProbe {
    id<MTLDevice> device = nil;
    id<MTLCommandQueue> queue = nil;
    void *filter = nullptr;
    id<MTLSharedEvent> gate = nil;
    CVMetalTextureCacheRef cache = nullptr;
    std::shared_ptr<Pool> pool = std::make_shared<Pool>();

    ~MetalProbe() {
        // Keep an error path from permanently stranding a gated command buffer.
        gate.signaledValue = 1;
        if (filter) vesper_probe_filter_close(filter);
        if (cache) CFRelease(cache);
    }
};

VesperProbeGpuContext gpu_context(const MetalProbe &probe, id<MTLCommandBuffer> command) noexcept {
    return {sizeof(VesperProbeGpuContext), VESPER_PROBE_GPU_METAL,
            (__bridge void *)probe.device, (__bridge void *)probe.queue,
            (__bridge void *)command, probe.cache};
}

void mark_retired(Pool &pool) noexcept {
    for (auto &slot : pool.slots) {
        auto state = SlotState::submitted;
        slot.state.compare_exchange_strong(state, SlotState::retired);
    }
}

bool import_plane(MetalProbe &probe, CVPixelBufferRef pixel_buffer, size_t plane,
                  MTLPixelFormat format, CVMetalTextureRef *out) noexcept {
    return CVMetalTextureCacheCreateTextureFromImage(
        kCFAllocatorDefault, probe.cache, pixel_buffer, nullptr, format,
        CVPixelBufferGetWidthOfPlane(pixel_buffer, plane),
        CVPixelBufferGetHeightOfPlane(pixel_buffer, plane), plane, out) == kCVReturnSuccess;
}

uint32_t submit(MetalProbe &probe, const VesperProbeVideoFrame &frame, bool gated) {
    auto pool = probe.pool;
    Slot *slot = nullptr;
    for (auto &candidate : pool->slots) {
        // The submitter is the sole claimant. Completion handlers only free slots.
        if (candidate.state.load() == SlotState::free) {
            slot = &candidate;
            break;
        }
    }
    if (!slot) return VESPER_PROBE_TRY_AGAIN;

    auto pixel_buffer = reinterpret_cast<CVPixelBufferRef>(static_cast<uintptr_t>(frame.pixel_buffer));
    if (!CVPixelBufferGetIOSurface(pixel_buffer) || CVPixelBufferGetPlaneCount(pixel_buffer) != 2
        || CVPixelBufferGetWidth(pixel_buffer) != frame.width
        || CVPixelBufferGetHeight(pixel_buffer) != frame.height) {
        return VESPER_PROBE_INVALID;
    }
    VesperProbeVideoFormat format{sizeof(VesperProbeVideoFormat), 0, frame.width, frame.height};
    MTLPixelFormat y_format, uv_format;
    switch (CVPixelBufferGetPixelFormatType(pixel_buffer)) {
        case kCVPixelFormatType_420YpCbCr8BiPlanarFullRange:
        case kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange:
            format.pixel_format = VESPER_PROBE_VIDEO_NV12;
            y_format = MTLPixelFormatR8Unorm;
            uv_format = MTLPixelFormatRG8Unorm;
            break;
        case kCVPixelFormatType_420YpCbCr10BiPlanarFullRange:
        case kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange:
            format.pixel_format = VESPER_PROBE_VIDEO_P010;
            y_format = MTLPixelFormatR16Unorm;
            uv_format = MTLPixelFormatRG16Unorm;
            break;
        default:
            return VESPER_PROBE_INVALID;
    }
    if (slot->target.width != frame.width || slot->target.height != frame.height) {
        auto desc = [MTLTextureDescriptor texture2DDescriptorWithPixelFormat:MTLPixelFormatRGBA8Unorm
                         width:frame.width height:frame.height mipmapped:NO];
        desc.storageMode = MTLStorageModePrivate;
        desc.usage = MTLTextureUsageShaderRead | MTLTextureUsageShaderWrite;
        slot->target = [probe.device newTextureWithDescriptor:desc];
        if (!slot->target) return VESPER_PROBE_INTERNAL;
    }

    CVMetalTextureRef y = nullptr;
    CVMetalTextureRef uv = nullptr;
    if (!import_plane(probe, pixel_buffer, 0, y_format, &y)
        || !import_plane(probe, pixel_buffer, 1, uv_format, &uv)) {
        if (y) CFRelease(y);
        if (uv) CFRelease(uv);
        return VESPER_PROBE_INTERNAL;
    }

    id<MTLCommandBuffer> command = [probe.queue commandBuffer];
    if (gated) [command encodeWaitForEvent:probe.gate value:1];
    const auto gpu = gpu_context(probe, command);
    const auto status = vesper_probe_filter_encode(probe.filter, &gpu, &format,
        (__bridge void *)CVMetalTextureGetTexture(y),
        (__bridge void *)CVMetalTextureGetTexture(uv), (__bridge void *)slot->target);
    if (status != VESPER_PROBE_OK) {
        CFRelease(y);
        CFRelease(uv);
        return status;
    }

    const auto release = frame.release_fn;
    const auto context = frame.release_ctx;
    const auto lease_id = frame.lease_id;
    slot->state.store(SlotState::submitted);
    ++pool->submitted;
    [command addCompletedHandler:^(id<MTLCommandBuffer> completed) {
        if (completed.status != MTLCommandBufferStatusCompleted) ++pool->failed;
        CFRelease(y);
        CFRelease(uv);
        release(context, lease_id);
        {
            std::lock_guard lock(pool->completion_lock);
            slot->state.store(SlotState::free);
            ++pool->released;
        }
        pool->completed.notify_all();
    }];
    [command commit];
    return VESPER_PROBE_OK;
}

} // namespace

extern "C" void *vesper_probe_metal_open() noexcept {
    @autoreleasepool {
        try {
            auto probe = std::make_unique<MetalProbe>();
            probe->device = MTLCreateSystemDefaultDevice();
            probe->queue = [probe->device newCommandQueue];
            probe->gate = [probe->device newSharedEvent];
            if (!probe->queue || !probe->gate) return nullptr;
            if (CVMetalTextureCacheCreate(kCFAllocatorDefault, nullptr, probe->device, nullptr,
                                         &probe->cache) != kCVReturnSuccess) return nullptr;
            const auto gpu = gpu_context(*probe, nil);
            probe->filter = vesper_probe_filter_open(&gpu);
            if (!probe->filter) return nullptr;
            return probe.release();
        } catch (...) {
            return nullptr;
        }
    }
}

extern "C" uint32_t vesper_probe_metal_submit(void *context, const VesperProbeVideoFrame *frame,
                                            uint32_t gated) noexcept {
    if (!context || !frame || frame->struct_size < sizeof(VesperProbeVideoFrame)
        || !frame->pixel_buffer || !frame->release_fn || !frame->release_ctx) {
        return VESPER_PROBE_INVALID;
    }
    @autoreleasepool {
        return submit(*static_cast<MetalProbe *>(context), *frame, gated != 0);
    }
}

extern "C" void vesper_probe_metal_retire(void *context) noexcept {
    mark_retired(*static_cast<MetalProbe *>(context)->pool);
}

extern "C" void vesper_probe_metal_signal(void *context) noexcept {
    static_cast<MetalProbe *>(context)->gate.signaledValue = 1;
}

extern "C" void vesper_probe_metal_stats(void *context, VesperProbeMetalStats *out) noexcept {
    const auto &pool = *static_cast<MetalProbe *>(context)->pool;
    *out = {pool.submitted, pool.released.load(), pool.failed.load(), 0, 0};
    for (const auto &slot : pool.slots) {
        switch (slot.state.load()) {
            case SlotState::submitted: ++out->active; break;
            case SlotState::retired: ++out->retired; break;
            case SlotState::free: break;
        }
    }
}

extern "C" uint32_t vesper_probe_metal_wait(void *context, uint32_t timeout_ms) noexcept {
    auto &pool = *static_cast<MetalProbe *>(context)->pool;
    std::unique_lock lock(pool.completion_lock);
    const bool idle = pool.completed.wait_for(lock, std::chrono::milliseconds(timeout_ms), [&] {
        return pool.released.load() == pool.submitted;
    });
    return idle ? VESPER_PROBE_OK : VESPER_PROBE_TRY_AGAIN;
}

extern "C" void vesper_probe_metal_close(void *context) noexcept {
    @autoreleasepool {
        auto *probe = static_cast<MetalProbe *>(context);
        mark_retired(*probe->pool);
        delete probe;
    }
}

extern "C" uint32_t vesper_probe_avplayer_check(const char *path) noexcept {
    @autoreleasepool {
        NSString *file = [NSString stringWithUTF8String:path];
        if (!file) return VESPER_PROBE_INVALID;
        AVPlayerItem *item = [AVPlayerItem playerItemWithURL:[NSURL fileURLWithPath:file]];
        AVPlayer *player = [AVPlayer playerWithPlayerItem:item];
        NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:5.0];
        while (player.currentItem.status == AVPlayerItemStatusUnknown
               && deadline.timeIntervalSinceNow > 0) {
            @autoreleasepool {
                [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.01]];
            }
        }
        if (player.currentItem.status == AVPlayerItemStatusReadyToPlay) return VESPER_PROBE_OK;
        NSLog(@"Metal probe AVPlayer item did not become ready: %@", item.error);
        return VESPER_PROBE_INTERNAL;
    }
}
