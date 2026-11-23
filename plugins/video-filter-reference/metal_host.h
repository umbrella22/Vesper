#ifndef VESPER_REFERENCE_METAL_HOST_H
#define VESPER_REFERENCE_METAL_HOST_H

#include <CoreVideo/CoreVideo.h>
#include "vesper_plugin.h"

#ifdef __cplusplus
extern "C" {
#endif

/* The host is confined to its presentation thread. The supplied parameters
 * use the reference filter's JSON format and are copied during creation. */
void *vesper_reference_metal_host_create(const void *device, const void *queue,
                                         CVMetalTextureCacheRef cache, const uint8_t *parameters, size_t parameters_len);
void vesper_reference_metal_host_destroy(void *host);

/* Optionally accept existing Metal plane imports. The returned texture is
 * borrowed by the same command buffer as the input and filter pass. */
VesperStatus vesper_reference_metal_host_encode(void *host, const void *command,
                                                CVPixelBufferRef pixel_buffer,
                                                const void *luma, const void *chroma,
                                                const void **out_texture);

#ifdef __cplusplus
}
#endif
#endif
