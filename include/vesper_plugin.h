#ifndef VESPER_PLUGIN_H
#define VESPER_PLUGIN_H

/* Generated from player-plugin-abi. Run vesper plugin header-generate to update. */

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#if defined(__cplusplus)
#define VESPER_NOEXCEPT noexcept
#else
#define VESPER_NOEXCEPT
#endif
#if defined(_WIN32) && defined(VESPER_PLUGIN_BUILD)
#define VESPER_PLUGIN_API __declspec(dllexport)
#elif defined(VESPER_PLUGIN_BUILD) && (defined(__GNUC__) || defined(__clang__))
#define VESPER_PLUGIN_API __attribute__((visibility("default")))
#else
#define VESPER_PLUGIN_API
#endif


#define VESPER_PLUGIN_ABI_MAJOR 2
#define VESPER_PLUGIN_ABI_MINOR 0
#define VESPER_MAX_INTERFACES_PER_PLUGIN 64
#define VESPER_MAX_LEASES_PER_SESSION 64

/**
 * Maximum UTF-8 byte length of a reverse-DNS plugin identity.
 */
#define VESPER_MAX_PLUGIN_ID_BYTES 255

/**
 * Maximum UTF-8 byte length of a human-readable plugin name.
 */
#define VESPER_MAX_PLUGIN_NAME_BYTES 255

/**
 * Maximum UTF-8 byte length of a capability instance identity.
 */
#define VESPER_MAX_CAPABILITY_INSTANCE_ID_BYTES 255

/**
 * Maximum size accepted for one borrowed source-normalizer packet payload.
 */
#define VESPER_MAX_PACKET_BYTES ((16 * 1024) * 1024)

#define VESPER_PIXEL_NV12 1
#define VESPER_PIXEL_P010 2
#define VESPER_PIXEL_YUV420P 3
#define VESPER_PIXEL_RGBA8 4
#define VESPER_PIXEL_BGRA8 5
#define VESPER_PIXEL_RGBA16F 6
#define VESPER_PIXEL_P016 7
#define VESPER_HDR_SDR 0
#define VESPER_HDR_HDR10 1
#define VESPER_HDR_HLG 2
#define VESPER_HDR_DOLBY_VISION 3
#define VESPER_SAMPLE_F32_PLANAR 1
#define VESPER_SAMPLE_F32_INTERLEAVED 2
#define VESPER_SAMPLE_S16_INTERLEAVED 3
#define VESPER_PACKET_KEYFRAME 1
#define VESPER_PACKET_DISCONTINUITY 2
#define VESPER_PACKET_EOS 4
#define VESPER_PACKET_CONFIG_CHANGED 8
#define VESPER_SEEK_BACKWARD 1
#define VESPER_HANDLE_CV_PIXEL_BUFFER 1
#define VESPER_HANDLE_MTL_TEXTURE 2
#define VESPER_HANDLE_AHB 3
#define VESPER_HANDLE_OH_NATIVE_BUFFER 4
#define VESPER_HANDLE_VK_IMAGE 5
#define VESPER_HANDLE_D3D11_TEXTURE2D 6
#define VESPER_HANDLE_DMABUF 7
#define VESPER_HANDLE_CPU_PLANES 8
#define VESPER_SYNC_NONE 0
#define VESPER_SYNC_MTL_EVENT 1
#define VESPER_SYNC_VK_TIMELINE_SEMAPHORE 2
#define VESPER_SYNC_FD 3
#define VESPER_RELEASE_RENDERED 1
#define VESPER_RELEASE_DROPPED 2
#define VESPER_GPU_NONE 0
#define VESPER_GPU_METAL 1
#define VESPER_GPU_VULKAN 2
#define VESPER_GPU_D3D11 3
#define VESPER_PITCH_PRESERVE 0
#define VESPER_PITCH_FOLLOW_RATE 1

/**
 * A fixed-layout borrowed slice. A null pointer is valid when len is zero.
 */
typedef struct VesperBytes {
  const uint8_t *data;
  uint64_t len;
} VesperBytes;

typedef uint32_t VesperStatus;

/**
 * Fixed-layout RFC 4122 bytes in network order.
 */
typedef struct VesperInterfaceId {
  uint8_t bytes[16];
} VesperInterfaceId;

/**
 * Successful entry returns a root valid until destroy_owner. Roots without
 * destroy_owner may be shared. Queries return immutable, root-owned tables.
 * Callers initialize output structs with their writable size. Writers may fill
 * only the ABI prefix they know. Failed opens leave the session output null.
 * Diagnostic slices and other borrowed JSON remain valid until the next call
 * to the same owning context. Unknown statuses are returned unchanged.
 */
typedef struct VesperPluginRoot {
  uint32_t struct_size;
  uint16_t abi_major;
  uint16_t abi_minor;
  void *owner;
  struct VesperBytes plugin_id;
  struct VesperBytes plugin_name;
  uint32_t interface_count;
  VesperStatus (*interface_at)(void *owner, uint32_t index, struct VesperInterfaceId *id) VESPER_NOEXCEPT;
  VesperStatus (*query_interface)(void *owner, const struct VesperInterfaceId *id, const void **out_vtable) VESPER_NOEXCEPT;
  void (*destroy_owner)(void *owner) VESPER_NOEXCEPT;
} VesperPluginRoot;

typedef VesperStatus (*VesperGetJson)(void *ctx, struct VesperBytes *out) VESPER_NOEXCEPT;

typedef struct VesperMetalContext {
  void *device;
  void *command_queue;
  void *command_buffer;
  void *texture_cache;
} VesperMetalContext;

typedef struct VesperVulkanContext {
  void *instance;
  void *physical_device;
  void *device;
  void *queue;
  void *command_buffer;
  const void *get_instance_proc_addr;
  uint32_t queue_family;
} VesperVulkanContext;

typedef struct VesperD3D11Context {
  void *device;
  void *immediate_context;
} VesperD3D11Context;

typedef union VesperGpuContextData {
  struct VesperMetalContext metal;
  struct VesperVulkanContext vulkan;
  struct VesperD3D11Context d3d11;
  uint64_t reserved[8];
} VesperGpuContextData;

/**
 * GPU objects are borrowed. Only the host submits the shared command buffer.
 */
typedef struct VesperGpuContext {
  uint32_t struct_size;
  uint32_t backend;
  union VesperGpuContextData data;
} VesperGpuContext;

/**
 * Session handles are exclusively owned and serialized by the host.
 */
typedef void VesperSession;

/**
 * ITU-T H.273 code points. Unspecified primaries, transfer or matrix use 2.
 */
typedef struct VesperColorInfo {
  uint8_t primaries;
  uint8_t transfer;
  uint8_t matrix;
  /**
   * 0 is unspecified, 1 is narrow, and 2 is full.
   */
  uint8_t range;
  uint8_t bit_depth;
  uint8_t hdr_kind;
  uint8_t reserved[2];
} VesperColorInfo;

/**
 * ST 2086 chromaticities use units of 1/50000. Luminance uses units of 1/10000 nit.
 */
typedef struct VesperHdrStaticMetadata {
  uint16_t display_primaries_x[3];
  uint16_t display_primaries_y[3];
  uint16_t white_point_x;
  uint16_t white_point_y;
  uint32_t max_luminance_nits_x10000;
  uint32_t min_luminance_nits_x10000;
  uint16_t max_cll;
  uint16_t max_fall;
} VesperHdrStaticMetadata;

typedef struct VesperVideoFormat {
  uint32_t struct_size;
  uint32_t pixel_format;
  /**
   * Starts at 1 and strictly increases when the session signals a format change.
   */
  uint64_t format_id;
  uint32_t width;
  uint32_t height;
  uint32_t coded_width;
  uint32_t coded_height;
  uint32_t crop_x;
  uint32_t crop_y;
  uint32_t crop_w;
  uint32_t crop_h;
  int16_t rotation_deg;
  uint8_t mirror_flags;
  uint8_t has_hdr_static;
  struct VesperColorInfo color;
  struct VesperHdrStaticMetadata hdr_static;
} VesperVideoFormat;

/**
 * Timestamps are in microseconds. INT64_MIN marks an unknown timestamp.
 */
typedef struct VesperPacket {
  uint32_t struct_size;
  uint32_t flags;
  int64_t pts_us;
  int64_t dts_us;
  int64_t duration_us;
  uint32_t stream_index;
  const uint8_t *data;
  uint64_t len;
} VesperPacket;

/**
 * Release each nonzero lease once, after GPU completion, even after session destruction.
 * The callback and its context must be valid on the completion thread.
 */
typedef struct VesperVideoFrame {
  uint32_t struct_size;
  uint32_t flags;
  uint64_t format_id;
  int64_t pts_us;
  int64_t duration_us;
  uint32_t handle_kind;
  uint32_t plane_count;
  uint64_t handles[3];
  uint32_t strides[3];
  uint32_t sync_kind;
  uint64_t sync_handle;
  uint64_t sync_value;
  uint64_t lease_id;
  void *release_ctx;
  void (*release_fn)(void *ctx, uint64_t lease_id, uint32_t disposition) VESPER_NOEXCEPT;
} VesperVideoFrame;

typedef VesperStatus (*VesperFlush)(VesperSession *session) VESPER_NOEXCEPT;

typedef void (*VesperDestroy)(VesperSession *session) VESPER_NOEXCEPT;

typedef const char *(*VesperGetLastError)(VesperSession *session, uint32_t *out_len) VESPER_NOEXCEPT;

typedef struct VesperVideoDecoderVtable {
  uint32_t struct_size;
  VesperGetJson query_capabilities_json;
  VesperStatus (*open)(void *ctx, const struct VesperGpuContext *gpu, uint32_t codec_fourcc, struct VesperBytes extradata, struct VesperBytes params, VesperSession **out_session) VESPER_NOEXCEPT;
  VesperStatus (*get_format)(VesperSession *session, struct VesperVideoFormat *out) VESPER_NOEXCEPT;
  VesperStatus (*send_packet)(VesperSession *session, const struct VesperPacket *packet) VESPER_NOEXCEPT;
  VesperStatus (*receive_frame)(VesperSession *session, struct VesperVideoFrame *out) VESPER_NOEXCEPT;
  VesperFlush flush;
  VesperDestroy destroy;
  /**
   * Optional UTF-8 detail, read on the failing call's thread before the next
   * decoder call. A null session requests the detail of an unsuccessful open.
   * The returned bytes are borrowed until the next decoder call on this thread.
   */
  VesperGetLastError get_last_error;
} VesperVideoDecoderVtable;

typedef struct VesperVideoFilterVtable {
  uint32_t struct_size;
  VesperGetJson query_capabilities_json;
  VesperStatus (*open)(void *ctx, const struct VesperGpuContext *gpu, const struct VesperVideoFormat *input, struct VesperBytes params, struct VesperVideoFormat *output, VesperSession **out_session) VESPER_NOEXCEPT;
  VesperStatus (*set_param)(VesperSession *session, struct VesperBytes key, struct VesperBytes value) VESPER_NOEXCEPT;
  /**
   * A null input drains the filter. PASSTHROUGH keeps the input lease.
   */
  VesperStatus (*process)(VesperSession *session, const struct VesperGpuContext *gpu, const struct VesperVideoFrame *input, struct VesperVideoFrame *out) VESPER_NOEXCEPT;
  VesperStatus (*get_format)(VesperSession *session, struct VesperVideoFormat *out) VESPER_NOEXCEPT;
  VesperFlush flush;
  VesperDestroy destroy;
  VesperGetLastError get_last_error;
} VesperVideoFilterVtable;

typedef struct VesperAudioFormat {
  uint32_t struct_size;
  uint32_t sample_format;
  uint64_t format_id;
  uint32_t sample_rate;
  uint16_t channels;
  uint16_t max_block_frames;
  uint64_t channel_layout;
} VesperAudioFormat;

/**
 * PCM is borrowed for this call. Input planes are read-only unless process_block
 * is called with the same writable descriptor as input and output. Each output
 * contains frames for exactly one rate_epoch, including during EOS drain.
 */
typedef struct VesperAudioBuffer {
  uint32_t struct_size;
  uint32_t flags;
  uint64_t format_id;
  int64_t pts_us;
  uint64_t rate_epoch;
  uint32_t frame_count;
  uint32_t frame_capacity;
  void *planes[8];
} VesperAudioBuffer;

typedef struct VesperAudioProcessorVtable {
  uint32_t struct_size;
  VesperGetJson query_capabilities_json;
  VesperStatus (*open)(void *ctx, const struct VesperAudioFormat *input, struct VesperBytes params, struct VesperAudioFormat *output, uint32_t *out_latency_frames, VesperSession **out_session) VESPER_NOEXCEPT;
  VesperStatus (*set_playback_policy)(VesperSession *session, float rate, uint32_t pitch_mode, uint64_t rate_epoch) VESPER_NOEXCEPT;
  VesperStatus (*set_param)(VesperSession *session, struct VesperBytes key, struct VesperBytes value) VESPER_NOEXCEPT;
  VesperStatus (*process_block)(VesperSession *session, const struct VesperAudioBuffer *input, struct VesperAudioBuffer *out) VESPER_NOEXCEPT;
  /**
   * A null input marks EOS. A non-null input may be partially consumed.
   */
  VesperStatus (*push)(VesperSession *session, const struct VesperAudioBuffer *input, uint32_t *out_consumed_frames) VESPER_NOEXCEPT;
  /**
   * EOF is returned only after buffered output from every epoch is drained.
   */
  VesperStatus (*pull)(VesperSession *session, struct VesperAudioBuffer *out, uint32_t *out_produced_frames) VESPER_NOEXCEPT;
  uint32_t (*latency_frames)(VesperSession *session) VESPER_NOEXCEPT;
  VesperFlush flush;
  VesperDestroy destroy;
  VesperGetLastError get_last_error;
} VesperAudioProcessorVtable;

typedef VesperStatus (*VesperOpenSource)(void *ctx, struct VesperBytes uri, struct VesperBytes params, VesperSession **out_session) VESPER_NOEXCEPT;

typedef struct VesperSourceNormalizerVtable {
  uint32_t struct_size;
  VesperGetJson query_capabilities_json;
  VesperOpenSource open;
  VesperStatus (*get_stream_info)(VesperSession *session, struct VesperBytes *out) VESPER_NOEXCEPT;
  /**
   * On OK the packet borrows bytes until its nonzero lease is released.
   */
  VesperStatus (*read_packet)(VesperSession *session, struct VesperPacket *out, uint64_t *out_lease_id) VESPER_NOEXCEPT;
  VesperStatus (*release_packet)(VesperSession *session, uint64_t lease_id) VESPER_NOEXCEPT;
  VesperStatus (*seek)(VesperSession *session, int64_t pts_us, uint32_t flags) VESPER_NOEXCEPT;
  VesperFlush flush;
  VesperDestroy destroy;
  VesperGetLastError get_last_error;
} VesperSourceNormalizerVtable;

/**
 * Resource production is independent of packet streaming. Destroy releases the
 * resource after the platform player has finished consuming it.
 */
typedef struct VesperSourceNormalizerResourceVtable {
  uint32_t struct_size;
  VesperGetJson query_capabilities_json;
  VesperOpenSource open;
  VesperStatus (*get_resource_json)(VesperSession *session, struct VesperBytes *out) VESPER_NOEXCEPT;
  VesperDestroy destroy;
  VesperGetLastError get_last_error;
  /**
   * Returns OK on change or TRY_AGAIN on timeout. The timeout is in milliseconds.
   */
  VesperStatus (*wait_for_update)(VesperSession *session, uint32_t timeout_ms) VESPER_NOEXCEPT;
  /**
   * Requests cancellation. OK means the worker stopped, TRY_AGAIN means cancellation is pending.
   */
  VesperStatus (*cancel)(VesperSession *session) VESPER_NOEXCEPT;
} VesperSourceNormalizerResourceVtable;

/**
 * The table and context are borrowed until the operation returns. Callbacks may
 * run on workers joined within the operation.
 */
typedef struct VesperProgressCallbacks {
  uint32_t struct_size;
  void *ctx;
  void (*progress)(void *ctx, uint64_t processed, uint64_t total) VESPER_NOEXCEPT;
  uint8_t (*is_cancelled)(void *ctx) VESPER_NOEXCEPT;
} VesperProgressCallbacks;

typedef VesperStatus (*VesperDownloadOperation)(void *ctx, struct VesperBytes request, const struct VesperProgressCallbacks *progress, struct VesperBytes *out) VESPER_NOEXCEPT;

typedef struct VesperPostDownloadProcessorVtable {
  uint32_t struct_size;
  /**
   * Capabilities JSON is immutable for the root lifetime.
   */
  VesperGetJson capabilities_json;
  VesperDownloadOperation process_json;
  VesperDownloadOperation assemble_json;
  /**
   * Releases a result returned by process_json or assemble_json.
   */
  void (*release_result)(void *ctx, struct VesperBytes result) VESPER_NOEXCEPT;
} VesperPostDownloadProcessorVtable;

#define VESPER_OK 0
#define VESPER_NEED_MORE_INPUT 1
#define VESPER_TRY_AGAIN 2
#define VESPER_EOF 3
#define VESPER_PASSTHROUGH 4
#define VESPER_FORMAT_CHANGED 5
#define VESPER_ERR_INVALID_ARG 100
#define VESPER_ERR_UNSUPPORTED 101
#define VESPER_ERR_INTERNAL 102
#define VESPER_ERR_GPU_LOST 103
#define VESPER_ERR_EXHAUSTED 104
#define VESPER_ERR_ABORTED 105
#define VESPER_ERR_PANIC 106

static const VesperInterfaceId VESPER_VIDEO_DECODER_ID = {{ 12, 66, 129, 91, 252, 50, 87, 26, 188, 234, 134, 121, 72, 41, 227, 155, }};

static const VesperInterfaceId VESPER_VIDEO_FILTER_ID = {{ 179, 185, 26, 8, 71, 33, 85, 164, 190, 103, 197, 89, 255, 163, 73, 241, }};

static const VesperInterfaceId VESPER_AUDIO_PROCESSOR_ID = {{ 166, 218, 143, 81, 228, 114, 81, 60, 159, 204, 154, 40, 178, 108, 164, 107, }};

static const VesperInterfaceId VESPER_SOURCE_NORMALIZER_ID = {{ 191, 185, 55, 253, 170, 44, 82, 105, 164, 146, 102, 153, 255, 29, 88, 250, }};

static const VesperInterfaceId VESPER_SOURCE_NORMALIZER_RESOURCE_ID = {{ 25, 70, 54, 114, 139, 66, 87, 201, 177, 247, 112, 245, 22, 93, 51, 113, }};

static const VesperInterfaceId VESPER_POST_DOWNLOAD_PROCESSOR_ID = {{ 0, 30, 225, 13, 76, 52, 82, 196, 189, 48, 98, 81, 99, 127, 71, 166, }};

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

VESPER_PLUGIN_API const struct VesperPluginRoot *vesper_native_plugin_entry(void) VESPER_NOEXCEPT;

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* VESPER_PLUGIN_H */
