#ifndef VESPER_PROBE_METAL_FILTER_H
#define VESPER_PROBE_METAL_FILTER_H

#include "plugin_probe_abi.h"

#ifdef __cplusplus
extern "C" {
#define VESPER_PROBE_NOEXCEPT noexcept
#else
#define VESPER_PROBE_NOEXCEPT
#endif

void *vesper_probe_filter_open(const VesperProbeGpuContext *context) VESPER_PROBE_NOEXCEPT;
uint32_t vesper_probe_filter_encode(void *filter, const VesperProbeGpuContext *context,
                                    const VesperProbeVideoFormat *format, void *y, void *uv,
                                    void *target) VESPER_PROBE_NOEXCEPT;
void vesper_probe_filter_close(void *filter) VESPER_PROBE_NOEXCEPT;

#undef VESPER_PROBE_NOEXCEPT
#ifdef __cplusplus
}
#endif

#endif
