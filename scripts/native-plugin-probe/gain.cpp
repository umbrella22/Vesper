#include "plugin_probe_abi.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <new>

namespace {

constexpr uint32_t fifo_frames = 256;

struct BufferedFrame {
    std::array<float, 2> samples;
    uint64_t epoch;
};

struct Gain {
    float multiplier;
    std::array<BufferedFrame, fifo_frames> fifo{};
    uint32_t head = 0;
    uint32_t length = 0;
    bool draining = false;
    uint64_t epoch = 0;
    uint32_t input_step = 1;
    uint32_t output_step = 1;
};

bool valid(const VesperProbeAudioBuffer *buffer) noexcept {
    return buffer && buffer->struct_size >= sizeof(VesperProbeAudioBuffer)
        && buffer->format_id == VESPER_PROBE_F32_STEREO_FORMAT
        && buffer->frame_count <= buffer->frame_capacity && buffer->planes[0];
}

uint32_t open(float gain, void **session) noexcept {
    if (!session) {
        return VESPER_PROBE_INVALID;
    }
    *session = nullptr;
    if (!std::isfinite(gain)) {
        return VESPER_PROBE_INVALID;
    }
    *session = new (std::nothrow) Gain{gain};
    return *session ? VESPER_PROBE_OK : VESPER_PROBE_INTERNAL;
}

uint32_t process_block(void *session, const VesperProbeAudioBuffer *input,
                       VesperProbeAudioBuffer *output) noexcept {
    if (!session || !valid(input) || !valid(output)
        || input->frame_count > output->frame_capacity) {
        return VESPER_PROBE_INVALID;
    }
    const auto *source = static_cast<const float *>(input->planes[0]);
    auto *target = static_cast<float *>(output->planes[0]);
    const auto samples = static_cast<size_t>(input->frame_count) * 2;
    const auto gain = static_cast<const Gain *>(session)->multiplier;
    for (size_t i = 0; i < samples; ++i) {
        target[i] = source[i] * gain;
    }
    output->frame_count = input->frame_count;
    return VESPER_PROBE_OK;
}

uint32_t push(void *session, const VesperProbeAudioBuffer *input, uint32_t *consumed) noexcept {
    if (!consumed) {
        return VESPER_PROBE_INVALID;
    }
    *consumed = 0;
    if (!session) {
        return VESPER_PROBE_INVALID;
    }
    auto &gain = *static_cast<Gain *>(session);
    if (!input) {
        gain.draining = true;
        return VESPER_PROBE_OK;
    }
    if (!valid(input) || !input->frame_count || gain.draining) {
        return VESPER_PROBE_INVALID;
    }
    const auto *samples = static_cast<const float *>(input->planes[0]);
    if (input->frame_count % gain.input_step != 0) {
        return VESPER_PROBE_INVALID;
    }
    // The rate fixture duplicates or drops whole frames in integral groups.
    // A production time stretcher can retain a larger internal window.
    const auto steps = std::min(input->frame_count / gain.input_step,
                                (fifo_frames - gain.length) / gain.output_step);
    if (steps == 0) {
        return VESPER_PROBE_TRY_AGAIN;
    }
    for (uint32_t step = 0; step < steps; ++step) {
        const auto offset = static_cast<size_t>(step) * gain.input_step * 2;
        const BufferedFrame frame{{samples[offset], samples[offset + 1]}, gain.epoch};
        for (uint32_t i = 0; i < gain.output_step; ++i) {
            gain.fifo[(gain.head + gain.length) % fifo_frames] = frame;
            ++gain.length;
        }
    }
    *consumed = steps * gain.input_step;
    return VESPER_PROBE_OK;
}

uint32_t pull(void *session, VesperProbeAudioBuffer *output, uint32_t *produced) noexcept {
    if (!produced) {
        return VESPER_PROBE_INVALID;
    }
    *produced = 0;
    if (!session || !valid(output) || !output->frame_capacity) {
        return VESPER_PROBE_INVALID;
    }
    auto &gain = *static_cast<Gain *>(session);
    const auto lookahead = static_cast<uint32_t>(!gain.draining);
    const auto ready = gain.length > lookahead ? gain.length - lookahead : 0;
    const auto capacity = std::min(output->frame_capacity, ready);
    output->frame_count = 0;
    output->rate_epoch = 0;
    if (capacity == 0) {
        return gain.draining ? VESPER_PROBE_EOF : VESPER_PROBE_NEED_INPUT;
    }
    auto *samples = static_cast<float *>(output->planes[0]);
    const auto epoch = gain.fifo[gain.head].epoch;
    uint32_t count = 0;
    // Each pull is attributable to one rate segment, even while several
    // segments are pending inside the processor.
    while (count < capacity && gain.fifo[gain.head].epoch == epoch) {
        const auto offset = static_cast<size_t>(count) * 2;
        const auto &frame = gain.fifo[gain.head].samples;
        samples[offset] = frame[0] * gain.multiplier;
        samples[offset + 1] = frame[1] * gain.multiplier;
        gain.head = (gain.head + 1) % fifo_frames;
        --gain.length;
        ++count;
    }
    output->rate_epoch = epoch;
    output->frame_count = count;
    *produced = count;
    return VESPER_PROBE_OK;
}

uint32_t flush(void *session) noexcept {
    if (!session) {
        return VESPER_PROBE_INVALID;
    }
    auto &gain = *static_cast<Gain *>(session);
    gain.head = gain.length = 0;
    gain.draining = false;
    return VESPER_PROBE_OK;
}

void destroy(void *session) noexcept {
    delete static_cast<Gain *>(session);
}

uint32_t set_playback_policy(void *session, float rate, uint64_t epoch) noexcept {
    if (!session || (rate != 0.5f && rate != 1.0f && rate != 2.0f)) {
        return VESPER_PROBE_INVALID;
    }
    auto &gain = *static_cast<Gain *>(session);
    if (gain.draining || epoch <= gain.epoch) {
        return VESPER_PROBE_INVALID;
    }
    gain.epoch = epoch;
    gain.input_step = rate == 2.0f ? 2 : 1;
    gain.output_step = rate == 0.5f ? 2 : 1;
    return VESPER_PROBE_OK;
}

uint32_t latency_frames(void *session) noexcept {
    return session ? static_cast<const Gain *>(session)->length : 0;
}

const VesperProbeGainApi fixture = {
    sizeof(VesperProbeGainApi),
    open, process_block, push, pull, flush, destroy, set_playback_policy, latency_frames,
};

} // namespace

extern "C" const VesperProbeGainApi *vesper_probe_gain_api() noexcept {
    return &fixture;
}
