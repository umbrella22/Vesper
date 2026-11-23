#include "vesper_plugin.h"

#include <cstddef>
#include <cstring>
#include <new>

namespace {

struct Session {
    VesperAudioFormat format;
};

template <std::size_t N>
VesperBytes bytes(const char (&text)[N]) noexcept {
    return {reinterpret_cast<const uint8_t *>(text), N - 1};
}

extern "C" VesperStatus capabilities(void *, VesperBytes *out) noexcept {
    if (!out) return VESPER_ERR_INVALID_ARG;
    *out = bytes(R"({"fixedBlock":true})");
    return VESPER_OK;
}

extern "C" VesperStatus open(void *, const VesperAudioFormat *input, VesperBytes,
                              VesperAudioFormat *output, uint32_t *latency,
                              VesperSession **session) noexcept {
    if (!session) return VESPER_ERR_INVALID_ARG;
    *session = nullptr;
    if (!input || input->struct_size < sizeof(VesperAudioFormat) || !output ||
        output->struct_size < sizeof(VesperAudioFormat) || !latency || input->format_id == 0 ||
        input->sample_format != VESPER_SAMPLE_F32_INTERLEAVED || !input->channels ||
        !input->max_block_frames || !input->sample_rate) {
        return VESPER_ERR_INVALID_ARG;
    }
    auto *state = new (std::nothrow) Session{*input};
    if (!state) return VESPER_ERR_EXHAUSTED;
    *output = state->format;
    output->struct_size = sizeof(VesperAudioFormat);
    *latency = 0;
    *session = state;
    return VESPER_OK;
}

extern "C" VesperStatus process(VesperSession *session, const VesperAudioBuffer *input,
                                 VesperAudioBuffer *output) noexcept {
    const auto *state = static_cast<const Session *>(session);
    if (!state || !input || input->struct_size < sizeof(VesperAudioBuffer) ||
        !output || output->struct_size < sizeof(VesperAudioBuffer) ||
        input->format_id != state->format.format_id || output->format_id != state->format.format_id ||
        input->frame_count > state->format.max_block_frames ||
        input->frame_count > input->frame_capacity || input->frame_count > output->frame_capacity) {
        return VESPER_ERR_INVALID_ARG;
    }
    const auto count = static_cast<std::size_t>(input->frame_count) * state->format.channels;
    const auto *source = static_cast<const float *>(input->planes[0]);
    auto *target = static_cast<float *>(output->planes[0]);
    if (count && (!source || !target || (input != output && source == target))) {
        return VESPER_ERR_INVALID_ARG;
    }
    for (std::size_t sample = 0; sample < count; ++sample) {
        target[sample] = source[sample] * 0.5f;
    }
    output->frame_count = input->frame_count;
    output->pts_us = input->pts_us;
    output->rate_epoch = input->rate_epoch;
    return VESPER_OK;
}

extern "C" VesperStatus policy(VesperSession *, float, uint32_t, uint64_t) noexcept {
    return VESPER_OK;
}

extern "C" VesperStatus flush(VesperSession *) noexcept { return VESPER_OK; }
extern "C" uint32_t latency(VesperSession *) noexcept { return 0; }
extern "C" void destroy(VesperSession *session) noexcept { delete static_cast<Session *>(session); }

const VesperAudioProcessorVtable audio{
    sizeof(VesperAudioProcessorVtable), capabilities, open, policy, nullptr, process,
    nullptr, nullptr, latency, flush, destroy, nullptr,
};

extern "C" VesperStatus interface_at(void *, uint32_t index, VesperInterfaceId *out) noexcept {
    if (index || !out) return VESPER_ERR_INVALID_ARG;
    *out = VESPER_AUDIO_PROCESSOR_ID;
    return VESPER_OK;
}

extern "C" VesperStatus query(void *, const VesperInterfaceId *id, const void **out) noexcept {
    if (!out) return VESPER_ERR_INVALID_ARG;
    *out = nullptr;
    if (!id || std::memcmp(id->bytes, VESPER_AUDIO_PROCESSOR_ID.bytes, sizeof(id->bytes))) {
        return VESPER_ERR_UNSUPPORTED;
    }
    *out = &audio;
    return VESPER_OK;
}

const VesperPluginRoot root{
    sizeof(VesperPluginRoot), VESPER_PLUGIN_ABI_MAJOR, VESPER_PLUGIN_ABI_MINOR,
    nullptr, bytes("dev.vesper.example.gain-cpp"), bytes("C++ Fixed Gain"),
    1, interface_at, query, nullptr,
};

}  // namespace

extern "C" VESPER_PLUGIN_API const VesperPluginRoot *vesper_native_plugin_entry() noexcept {
    return &root;
}
