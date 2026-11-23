#include "vesper_plugin.h"

#include <algorithm>
#include <array>
#include <charconv>
#include <cmath>
#include <cstring>
#include <limits>
#include <memory>
#include <string_view>
#include <vector>

namespace {

constexpr uint16_t max_channels = 8;
constexpr uint16_t max_block_frames = 8192;
constexpr auto unknown_pts = std::numeric_limits<int64_t>::min();
constexpr std::string_view plugin_id = "io.github.umbrella22.vesper-wsola";
constexpr std::string_view plugin_name = "Vesper Gain and WSOLA Reference";
constexpr std::string_view capabilities_json = R"({"sampleFormat":"f32-interleaved","fixedBlock":true,"buffered":true,"rateRange":[0.5,2.0]})";

VesperBytes bytes(std::string_view value) noexcept {
    return {reinterpret_cast<const uint8_t *>(value.data()), value.size()};
}

struct Policy {
    float rate = 1.0f;
    uint64_t epoch = 1;
};

// A segment has one rate and one source time origin. New-epoch pushes wait for
// the old tail to drain. The ring, crossfade window and output hop are allocated
// together with the session. WAV length and chunking do not affect storage size.
struct Session {
    VesperAudioFormat format;
    uint32_t hop;
    uint32_t search;
    uint32_t ring_frames;
    std::vector<float> ring;
    std::vector<float> previous_tail;
    std::vector<float> pending;
    std::array<float, max_channels> low{};
    float gain = 1.0f;
    float bass = 0.0f;
    Policy selected;
    Policy segment;
    bool active = false;
    bool finishing = false;
    bool eos = false;
    bool has_tail = false;
    int64_t origin = unknown_pts;
    uint64_t first = 0;
    uint64_t written = 0;
    uint64_t synthesized = 0;
    uint32_t pending_count = 0;
    uint32_t pending_offset = 0;

    explicit Session(const VesperAudioFormat &format)
        : format(format), hop(format.sample_rate / 100), search(format.sample_rate / 400),
          ring_frames(format.max_block_frames + 4 * hop + 2 * search),
          ring(static_cast<size_t>(ring_frames) * format.channels),
          previous_tail(static_cast<size_t>(hop) * format.channels),
          pending(static_cast<size_t>(hop) * format.channels) {}

    size_t offset(uint64_t frame, uint16_t channel) const noexcept {
        return static_cast<size_t>(frame) * format.channels + channel;
    }

    float sample(uint64_t frame, uint16_t channel) const noexcept {
        frame = std::min(frame, written - 1);
        return ring[offset(frame % ring_frames, channel)];
    }

    float equalize(float value, uint16_t ch) noexcept {
        low[ch] += 0.025f * (value - low[ch]);
        return gain * (value + bass * low[ch]);
    }

    void reset_segment() noexcept {
        active = finishing = has_tail = false;
        origin = unknown_pts;
        first = written = synthesized = 0;
        pending_count = pending_offset = 0;
    }

    int64_t media_pts(uint64_t frame) const noexcept {
        if (origin == unknown_pts) {
            return origin;
        }
        const auto offset_us = static_cast<long double>(frame) * segment.rate * 1000000 / format.sample_rate;
        const auto target = static_cast<long double>(origin) + offset_us;
        return target >= static_cast<long double>(INT64_MAX) ? unknown_pts : static_cast<int64_t>(target);
    }

    uint64_t best_window(uint64_t nominal) const noexcept {
        if (!has_tail) {
            return nominal;
        }
        const auto low_bound = nominal > search ? nominal - search : 0;
        const auto begin = std::max(first, low_bound);
        const auto end = std::min(written - 1, nominal + search);
        auto best = std::clamp(nominal, begin, end);
        double best_score = -std::numeric_limits<double>::infinity();
        // Estimate similarity on decimated PCM, then retain the full-resolution
        // frames for the overlap-add. This keeps the search bounded at open.
        for (auto candidate = begin; candidate <= end; ++candidate) {
            double dot = 0, input_power = 0, tail_power = 0;
            for (uint32_t frame = 0; frame < hop; frame += 4) {
                for (uint16_t ch = 0; ch < format.channels; ++ch) {
                    const double a = sample(candidate + frame, ch);
                    const double b = previous_tail[offset(frame, ch)];
                    dot += a * b;
                    input_power += a * a;
                    tail_power += b * b;
                }
            }
            const auto distance = candidate > nominal ? candidate - nominal : nominal - candidate;
            const double score = dot / std::sqrt(1e-16 + input_power * tail_power)
                - 0.001 * static_cast<double>(distance) / search;
            if (score > best_score) {
                best_score = score;
                best = candidate;
            }
        }
        return best;
    }

    bool prepare_hop() noexcept {
        const auto target = static_cast<uint64_t>(std::llround(written / static_cast<double>(segment.rate)));
        if (synthesized >= target) {
            return false;
        }
        auto nominal = static_cast<uint64_t>(std::llround(synthesized * static_cast<double>(segment.rate)));
        if (!finishing && (target - synthesized < hop || nominal + search + 2 * hop > written)) {
            return false;
        }
        nominal = std::clamp(nominal, first, written - 1);
        const auto start = best_window(nominal);
        pending_count = static_cast<uint32_t>(std::min<uint64_t>(hop, target - synthesized));
        pending_offset = 0;
        for (uint32_t frame = 0; frame < hop; ++frame) {
            const float weight = static_cast<float>(frame + 1) / hop;
            for (uint16_t ch = 0; ch < format.channels; ++ch) {
                const auto index = offset(frame, ch);
                const auto value = sample(start + frame, ch);
                pending[index] = has_tail ? previous_tail[index] * (1 - weight) + value * weight : value;
                previous_tail[index] = sample(start + hop + frame, ch);
            }
        }
        has_tail = true;
        synthesized += pending_count;
        const auto next = static_cast<uint64_t>(std::llround(synthesized * static_cast<double>(segment.rate)));
        first = std::max(first, std::min(written - 1, next > search ? next - search : 0));
        return true;
    }
};

bool valid(const Session &session, const VesperAudioBuffer *buffer) noexcept {
    return buffer && buffer->struct_size >= sizeof(VesperAudioBuffer)
        && buffer->format_id == session.format.format_id && buffer->frame_count <= buffer->frame_capacity
        && buffer->frame_capacity <= session.format.max_block_frames
        && (buffer->frame_capacity == 0 || buffer->planes[0]);
}

VesperStatus capabilities(void *, VesperBytes *out) noexcept {
    if (!out) return VESPER_ERR_INVALID_ARG;
    *out = bytes(capabilities_json);
    return VESPER_OK;
}

VesperStatus open(void *, const VesperAudioFormat *input, VesperBytes,
                  VesperAudioFormat *output, uint32_t *latency, VesperSession **session) noexcept {
    if (!session || !latency) return VESPER_ERR_INVALID_ARG;
    *session = nullptr;
    *latency = 0;
    if (!input || input->struct_size < sizeof(VesperAudioFormat) ||
        !output || output->struct_size < sizeof(VesperAudioFormat)) return VESPER_ERR_INVALID_ARG;
    if (input->sample_format != VESPER_SAMPLE_F32_INTERLEAVED ||
        !input->channels || input->channels > max_channels || input->sample_rate < 8000 ||
        input->sample_rate > 192000 || !input->format_id || !input->max_block_frames ||
        input->max_block_frames > max_block_frames) return VESPER_ERR_UNSUPPORTED;
    try {
        auto state = std::make_unique<Session>(*input);
        *output = *input;
        output->struct_size = sizeof(VesperAudioFormat);
        *latency = state->hop * 2 + state->search;
        *session = state.release();
        return VESPER_OK;
    } catch (...) {
        return VESPER_ERR_EXHAUSTED;
    }
}

VesperStatus policy(VesperSession *raw, float rate, uint32_t pitch, uint64_t epoch) noexcept {
    if (!raw || !std::isfinite(rate) || !epoch) return VESPER_ERR_INVALID_ARG;
    if (rate < 0.5f || rate > 2.0f || (pitch != VESPER_PITCH_PRESERVE && rate != 1.0f)) {
        return VESPER_ERR_UNSUPPORTED;
    }
    auto &session = *static_cast<Session *>(raw);
    if (session.eos || epoch < session.selected.epoch ||
        (session.active && epoch == session.segment.epoch && rate != session.segment.rate)) return VESPER_ERR_INVALID_ARG;
    session.selected = {rate, epoch};
    return VESPER_OK;
}

VesperStatus parameter(VesperSession *raw, VesperBytes key, VesperBytes value) noexcept {
    if (!raw || !key.data || !value.data || key.len > 16 || !value.len || value.len > 32) return VESPER_ERR_INVALID_ARG;
    const std::string_view name(reinterpret_cast<const char *>(key.data), key.len);
    const auto *begin = reinterpret_cast<const char *>(value.data);
    const auto *end = begin + value.len;
    float setting = 0;
    auto parsed = std::from_chars(begin, end, setting);
    if (parsed.ec != std::errc{} || parsed.ptr != end || !std::isfinite(setting)) return VESPER_ERR_INVALID_ARG;
    auto &session = *static_cast<Session *>(raw);
    if (name == "gain" && setting >= 0 && setting <= 4) {
        session.gain = setting;
        return VESPER_OK;
    }
    if (name == "bass" && setting >= -1 && setting <= 2) {
        session.bass = setting;
        return VESPER_OK;
    }
    return VESPER_ERR_UNSUPPORTED;
}

VesperStatus block(VesperSession *raw, const VesperAudioBuffer *input, VesperAudioBuffer *output) noexcept {
    if (!raw) return VESPER_ERR_INVALID_ARG;
    auto &session = *static_cast<Session *>(raw);
    if (!valid(session, input) || !valid(session, output) || input->frame_count > output->frame_capacity) {
        return VESPER_ERR_INVALID_ARG;
    }
    const auto *source = static_cast<const float *>(input->planes[0]);
    auto *target = static_cast<float *>(output->planes[0]);
    for (uint32_t frame = 0; frame < input->frame_count; ++frame) {
        for (uint16_t ch = 0; ch < session.format.channels; ++ch) {
            const auto i = session.offset(frame, ch);
            target[i] = session.equalize(source[i], ch);
        }
    }
    output->frame_count = input->frame_count;
    output->pts_us = input->pts_us;
    output->rate_epoch = input->rate_epoch;
    return VESPER_OK;
}

VesperStatus push(VesperSession *raw, const VesperAudioBuffer *input, uint32_t *consumed) noexcept {
    if (!raw || !consumed) return VESPER_ERR_INVALID_ARG;
    *consumed = 0;
    auto &session = *static_cast<Session *>(raw);
    if (!input) {
        session.finishing = session.eos = true;
        return VESPER_OK;
    }
    if (!valid(session, input) || !input->frame_count || session.eos) return VESPER_ERR_INVALID_ARG;
    if (session.active && input->rate_epoch != session.segment.epoch) {
        if (input->rate_epoch != session.selected.epoch) return VESPER_ERR_INVALID_ARG;
        session.finishing = true;
        return VESPER_TRY_AGAIN;
    }
    if (!session.active) {
        if (input->rate_epoch != session.selected.epoch) return VESPER_ERR_INVALID_ARG;
        session.active = true;
        session.segment = session.selected;
        session.origin = input->pts_us;
    }
    // Keep absolute counters in the exact integer range of the WSOLA position math.
    if (session.written > (uint64_t{1} << 50)) return VESPER_ERR_EXHAUSTED;
    const auto available = session.ring_frames - (session.written - session.first);
    const auto count = static_cast<uint32_t>(std::min<uint64_t>(available, input->frame_count));
    if (!count) return VESPER_TRY_AGAIN;
    const auto *source = static_cast<const float *>(input->planes[0]);
    for (uint32_t frame = 0; frame < count; ++frame) {
        const auto index = session.offset((session.written + frame) % session.ring_frames, 0);
        std::copy_n(source + session.offset(frame, 0), session.format.channels, session.ring.data() + index);
    }
    session.written += count;
    *consumed = count;
    return VESPER_OK;
}

VesperStatus pull(VesperSession *raw, VesperAudioBuffer *out, uint32_t *produced) noexcept {
    if (!raw || !produced) return VESPER_ERR_INVALID_ARG;
    *produced = 0;
    auto &session = *static_cast<Session *>(raw);
    if (!valid(session, out) || !out->frame_capacity) return VESPER_ERR_INVALID_ARG;
    if (!session.active) return session.eos ? VESPER_EOF : VESPER_NEED_MORE_INPUT;
    auto *target = static_cast<float *>(out->planes[0]);
    uint64_t start;
    uint32_t count;
    if (session.segment.rate == 1.0f) {
        start = session.synthesized;
        count = static_cast<uint32_t>(std::min<uint64_t>(out->frame_capacity, session.written - start));
        for (uint32_t frame = 0; frame < count; ++frame) {
            for (uint16_t ch = 0; ch < session.format.channels; ++ch) {
                target[session.offset(frame, ch)] = session.equalize(session.sample(start + frame, ch), ch);
            }
        }
        session.synthesized += count;
        session.first = session.synthesized;
    } else {
        if (session.pending_count == session.pending_offset && !session.prepare_hop()) {
            if (session.finishing) session.reset_segment();
            return session.eos ? VESPER_EOF : VESPER_NEED_MORE_INPUT;
        }
        start = session.synthesized - session.pending_count + session.pending_offset;
        count = std::min(out->frame_capacity, session.pending_count - session.pending_offset);
        for (uint32_t frame = 0; frame < count; ++frame) {
            for (uint16_t ch = 0; ch < session.format.channels; ++ch) {
                target[session.offset(frame, ch)] = session.equalize(session.pending[session.offset(session.pending_offset + frame, ch)], ch);
            }
        }
        session.pending_offset += count;
    }
    if (!count) {
        if (session.finishing) session.reset_segment();
        return session.eos ? VESPER_EOF : VESPER_NEED_MORE_INPUT;
    }
    out->pts_us = session.media_pts(start);
    out->rate_epoch = session.segment.epoch;
    out->frame_count = count;
    *produced = count;
    return VESPER_OK;
}

uint32_t latency(VesperSession *raw) noexcept {
    auto *session = static_cast<const Session *>(raw);
    return session ? session->hop * 2 + session->search : 0;
}

VesperStatus flush(VesperSession *raw) noexcept {
    if (!raw) return VESPER_ERR_INVALID_ARG;
    auto &session = *static_cast<Session *>(raw);
    session.reset_segment();
    session.eos = false;
    session.low.fill(0);
    return VESPER_OK;
}

void destroy(VesperSession *raw) noexcept { delete static_cast<Session *>(raw); }

const VesperAudioProcessorVtable table = {
    sizeof(VesperAudioProcessorVtable), capabilities, open, policy, parameter, block,
    push, pull, latency, flush, destroy, nullptr,
};

VesperStatus interface_at(void *, uint32_t index, VesperInterfaceId *out) noexcept {
    if (index || !out) return VESPER_ERR_INVALID_ARG;
    *out = VESPER_AUDIO_PROCESSOR_ID;
    return VESPER_OK;
}
VesperStatus query(void *, const VesperInterfaceId *id, const void **out) noexcept {
    if (!id || !out) return VESPER_ERR_INVALID_ARG;
    *out = nullptr;
    if (std::memcmp(id->bytes, VESPER_AUDIO_PROCESSOR_ID.bytes, sizeof(id->bytes))) return VESPER_ERR_UNSUPPORTED;
    *out = &table;
    return VESPER_OK;
}

const VesperPluginRoot root = {
    sizeof(VesperPluginRoot), VESPER_PLUGIN_ABI_MAJOR, VESPER_PLUGIN_ABI_MINOR,
    nullptr, bytes(plugin_id), bytes(plugin_name), 1, interface_at, query, nullptr,
};

} // namespace

const VesperPluginRoot *vesper_native_plugin_entry() noexcept { return &root; }
