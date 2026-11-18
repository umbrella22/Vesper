// Copyright 2026 Vesper contributors. SPDX-License-Identifier: Apache-2.0
#include "vesper_harmony.h"
#ifdef VESPER_HARMONY_HOST_TEST
#include <node_api.h>
#else
#include <napi/native_api.h>
#include <multimedia/player_framework/native_avcapability.h>
#endif
#include <array>
#include <charconv>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <limits>
#include <memory>
#include <new>
#include <set>
#include <string>

namespace {
constexpr size_t MaxBytes = 1024 * 1024;
struct Environment { std::set<uint64_t> handles; };

napi_value Fail(napi_env env, const char* code, const char* message)
{
    napi_throw_error(env, code, message);
    return nullptr;
}

napi_value NativeFailure(napi_env env, VesperHarmonyStatus status)
{
    const std::string code = std::to_string(static_cast<uint32_t>(status));
    return Fail(env, code.c_str(), "Vesper native bridge rejected the operation");
}

bool ReadString(napi_env env, napi_value value, std::string& output, size_t maximum)
{
    napi_valuetype type;
    size_t size = 0;
    if (napi_typeof(env, value, &type) != napi_ok || type != napi_string ||
        napi_get_value_string_utf8(env, value, nullptr, 0, &size) != napi_ok || size > maximum) {
        Fail(env, "invalid_argument", "Expected a bounded UTF-8 string");
        return false;
    }
    output.resize(size + 1);
    size_t copied = 0;
    if (napi_get_value_string_utf8(env, value, output.data(), output.size(), &copied) != napi_ok || copied != size) {
        Fail(env, "invalid_argument", "UTF-8 input could not be copied");
        return false;
    }
    output.resize(size);
    return true;
}

bool ReadHandle(napi_env env, napi_value value, uint64_t& handle)
{
    std::string text;
    if (!ReadString(env, value, text, 20)) { return false; }
    if (text.empty() || text.find_first_not_of("0123456789") != std::string::npos) {
        Fail(env, "invalid_argument", "Expected a decimal u64 identity");
        return false;
    }
    const auto parsed = std::from_chars(text.data(), text.data() + text.size(), handle);
    if (parsed.ec != std::errc{} || parsed.ptr != text.data() + text.size() || handle == 0) {
        Fail(env, "invalid_argument", "Invalid decimal u64 identity");
        return false;
    }
    return true;
}

bool Arguments(napi_env env, napi_callback_info info, size_t expected,
    std::array<napi_value, 3>& values, Environment*& state)
{
    size_t count = values.size();
    void* data = nullptr;
    if (napi_get_cb_info(env, info, &count, values.data(), nullptr, &data) != napi_ok ||
        count != expected || data == nullptr) {
        Fail(env, "invalid_argument", "Unexpected bridge arguments");
        return false;
    }
    state = static_cast<Environment*>(data);
    return true;
}

napi_value Create(napi_env env, napi_callback_info info)
{
    std::array<napi_value, 3> args{};
    Environment* state = nullptr;
    if (!Arguments(env, info, 1, args, state)) { return nullptr; }
    if (state->handles.size() >= 64) { return Fail(env, "capacity", "Environment session capacity reached"); }
    std::string config;
    if (!ReadString(env, args[0], config, MaxBytes)) { return nullptr; }
    const auto reply = vesper_harmony_create(reinterpret_cast<const uint8_t*>(config.data()), config.size());
    if (reply.status != Ok) { return NativeFailure(env, reply.status); }
    const auto identity = std::to_string(reply.value);
    napi_value result;
    if (napi_create_string_utf8(env, identity.data(), identity.size(), &result) != napi_ok) {
        vesper_harmony_release(reply.value);
        return Fail(env, "allocation", "Identity allocation failed");
    }
    state->handles.insert(reply.value);
    return result;
}

struct ResponseOwner {
    uint64_t id;
    ~ResponseOwner() { vesper_harmony_buffer_release(id); }
};

napi_value Call(napi_env env, napi_callback_info info)
{
    std::array<napi_value, 3> args{};
    Environment* state = nullptr;
    if (!Arguments(env, info, 2, args, state)) { return nullptr; }
    uint64_t handle = 0;
    std::string command;
    if (!ReadHandle(env, args[0], handle) || !ReadString(env, args[1], command, MaxBytes)) { return nullptr; }
    if (state->handles.count(handle) == 0) { return Fail(env, "stale_handle", "Session is not owned by this environment"); }
    const auto now = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();
    const auto reply = vesper_harmony_call(handle, reinterpret_cast<const uint8_t*>(command.data()),
        command.size(), now > 0 ? static_cast<uint64_t>(now) : 0);
    if (reply.status != Ok) { return NativeFailure(env, reply.status); }
    const ResponseOwner owner{reply.value};
    const auto size = vesper_harmony_buffer_read(owner.id, nullptr, 0);
    if (size.status != BufferTooSmall || size.value == 0 || size.value > MaxBytes) {
        return Fail(env, "invalid_response", "Native response size was invalid");
    }
    auto bytes = std::unique_ptr<uint8_t[]>(new (std::nothrow) uint8_t[size.value]);
    if (!bytes) { return Fail(env, "allocation", "Response allocation failed"); }
    const auto copied = vesper_harmony_buffer_read(owner.id, bytes.get(), size.value);
    if (copied.status != Ok || copied.value != size.value) { return NativeFailure(env, copied.status); }
    napi_value result;
    if (napi_create_string_utf8(env, reinterpret_cast<const char*>(bytes.get()), copied.value, &result) != napi_ok) {
        return Fail(env, "allocation", "Response string allocation failed");
    }
    return result;
}

napi_value Release(napi_env env, napi_callback_info info)
{
    std::array<napi_value, 3> args{};
    Environment* state = nullptr;
    if (!Arguments(env, info, 1, args, state)) { return nullptr; }
    uint64_t handle = 0;
    if (!ReadHandle(env, args[0], handle)) { return nullptr; }
    if (state->handles.erase(handle) > 0) { vesper_harmony_release(handle); }
    napi_value result;
    if (napi_get_undefined(env, &result) != napi_ok) { return nullptr; }
    return result;
}

bool StringField(napi_env env, napi_value object, const char* key, const char* value)
{
    napi_value field;
    const size_t size = strnlen(value, 1025);
    return size <= 1024 && napi_create_string_utf8(env, value, size, &field) == napi_ok &&
        napi_set_named_property(env, object, key, field) == napi_ok;
}

[[maybe_unused]] bool NumberField(napi_env env, napi_value object, const char* key, int32_t value)
{
    napi_value field;
    return napi_create_int32(env, value, &field) == napi_ok &&
        napi_set_named_property(env, object, key, field) == napi_ok;
}

[[maybe_unused]] bool BooleanField(napi_env env, napi_value object, const char* key, bool value)
{
    napi_value field;
    return napi_get_boolean(env, value, &field) == napi_ok &&
        napi_set_named_property(env, object, key, field) == napi_ok;
}

// Queries framework-owned capability metadata; no codec or player is created.
napi_value QueryVideoDecoder(napi_env env, napi_callback_info info)
{
    size_t count = 5;
    std::array<napi_value, 5> args{};
    if (napi_get_cb_info(env, info, &count, args.data(), nullptr, nullptr) != napi_ok || count != 5) {
        return Fail(env, "invalid_argument", "Expected MIME, width, height, profile and level");
    }
    std::string mime;
    if (!ReadString(env, args[0], mime, 128)) { return nullptr; }
    if (mime != "video/avc" && mime != "video/hevc" && mime != "video/x-vnd.on2.vp8" &&
        mime != "video/x-vnd.on2.vp9" && mime != "video/av01") {
        return Fail(env, "unsupported", "Unsupported video capability MIME type");
    }
    std::array<int32_t, 4> values{};
    for (size_t index = 0; index < values.size(); ++index) {
        double number;
        if (napi_get_value_double(env, args[index + 1], &number) != napi_ok || !std::isfinite(number) ||
            number != std::floor(number) || number < -1 || number > std::numeric_limits<int32_t>::max()) {
            return Fail(env, "invalid_argument", "Capability dimensions and profile require int32 values");
        }
        values[index] = static_cast<int32_t>(number);
    }
    const auto width = values[0], height = values[1], profile = values[2], level = values[3];
    if (width < 0 || height < 0 || (width == 0) != (height == 0) || (profile < 0) != (level < 0)) {
        return Fail(env, "invalid_argument", "Capability size and profile/level must be paired");
    }
    napi_value result;
    if (napi_create_object(env, &result) != napi_ok || !StringField(env, result, "mimeType", mime.c_str()) ||
        !StringField(env, result, "decoderExecutionEvidence", "unknown")) {
        return Fail(env, "allocation", "Capability result allocation failed");
    }
#ifdef VESPER_HARMONY_HOST_TEST
    if (!StringField(env, result, "status", "unavailable") ||
        !StringField(env, result, "reason", "Hardware capability queries require the Harmony runtime")) {
        return Fail(env, "allocation", "Capability result allocation failed");
    }
#else
    // This pointer and its property arrays are borrowed from the media framework.
    auto* capability = OH_AVCodec_GetCapabilityByCategory(mime.c_str(), false, HARDWARE);
    if (capability == nullptr || !OH_AVCapability_IsHardware(capability)) {
        if (!StringField(env, result, "status", "unavailable") ||
            !StringField(env, result, "reason", "No matching hardware decoder capability was returned")) {
            return Fail(env, "allocation", "Capability result allocation failed");
        }
        return result;
    }
    const char* name = OH_AVCapability_GetName(capability);
    if (!StringField(env, result, "status", "available") || !BooleanField(env, result, "hardware", true) ||
        (name != nullptr && !StringField(env, result, "name", name))) {
        return Fail(env, "allocation", "Capability result allocation failed");
    }
    if (width > 0 && !BooleanField(env, result, "sizeSupported", OH_AVCapability_IsVideoSizeSupported(capability, width, height))) {
        return Fail(env, "allocation", "Capability result allocation failed");
    }
    if (profile >= 0 && !BooleanField(env, result, "profileAndLevelSupported",
        OH_AVCapability_AreProfileAndLevelSupported(capability, profile, level))) {
        return Fail(env, "allocation", "Capability result allocation failed");
    }
    OH_AVRange range{};
    const auto rangeField = [&](const char* key, OH_AVErrCode status) {
        if (status != AV_ERR_OK) { return true; }
        napi_value object;
        return napi_create_object(env, &object) == napi_ok && NumberField(env, object, "min", range.minVal) &&
            NumberField(env, object, "max", range.maxVal) && napi_set_named_property(env, result, key, object) == napi_ok;
    };
    if (!rangeField("widthRange", OH_AVCapability_GetVideoWidthRange(capability, &range)) ||
        !rangeField("heightRange", OH_AVCapability_GetVideoHeightRange(capability, &range)) ||
        !rangeField("frameRateRange", width > 0 ? OH_AVCapability_GetVideoFrameRateRangeForSize(capability, width, height, &range) :
            OH_AVCapability_GetVideoFrameRateRange(capability, &range))) {
        return Fail(env, "allocation", "Capability result allocation failed");
    }
    const int32_t* profiles = nullptr;
    uint32_t profileCount = 0;
    if (OH_AVCapability_GetSupportedProfiles(capability, &profiles, &profileCount) == AV_ERR_OK) {
        if (profileCount > 256 || (profileCount > 0 && profiles == nullptr)) {
            return Fail(env, "capacity", "Native capability profile list exceeds 256 entries");
        }
        napi_value array;
        if (napi_create_array_with_length(env, profileCount, &array) != napi_ok) { return Fail(env, "allocation", "Profile allocation failed"); }
        for (uint32_t index = 0; index < profileCount; ++index) {
            napi_value value;
            if (napi_create_int32(env, profiles[index], &value) != napi_ok || napi_set_element(env, array, index, value) != napi_ok) {
                return Fail(env, "allocation", "Profile allocation failed");
            }
        }
        if (napi_set_named_property(env, result, "profiles", array) != napi_ok) { return Fail(env, "allocation", "Profile allocation failed"); }
    }
#endif
    return result;
}

void Cleanup(void* data)
{
    auto* state = static_cast<Environment*>(data);
    // All bridge operations are synchronous, bounded CPU work on this environment.
    // No worker, platform player or JS reference survives this cleanup hook.
    for (const auto handle : state->handles) { vesper_harmony_release(handle); }
    delete state;
}

napi_value Initialize(napi_env env, napi_value exports)
{
    if (vesper_harmony_abi_version() != 1) { return Fail(env, "abi_version", "Native ABI version mismatch"); }
    auto* state = new (std::nothrow) Environment;
    if (state == nullptr) { return Fail(env, "allocation", "Environment allocation failed"); }
    if (napi_add_env_cleanup_hook(env, Cleanup, state) != napi_ok) {
        delete state;
        return Fail(env, "cleanup", "Environment cleanup registration failed");
    }
    const napi_property_descriptor properties[] = {
        {"create", nullptr, Create, nullptr, nullptr, nullptr, napi_default, state},
        {"call", nullptr, Call, nullptr, nullptr, nullptr, napi_default, state},
        {"release", nullptr, Release, nullptr, nullptr, nullptr, napi_default, state},
        {"queryVideoDecoder", nullptr, QueryVideoDecoder, nullptr, nullptr, nullptr, napi_default, state},
    };
    if (napi_define_properties(env, exports, sizeof(properties) / sizeof(properties[0]), properties) != napi_ok) {
        napi_remove_env_cleanup_hook(env, Cleanup, state);
        delete state;
        return Fail(env, "exports", "Native export registration failed");
    }
    return exports;
}
} // namespace

#ifdef VESPER_HARMONY_HOST_TEST
NAPI_MODULE(vesper_player_harmony_node, Initialize)
#else
static napi_module module = {
    .nm_version = 1, .nm_flags = 0, .nm_filename = nullptr,
    .nm_register_func = Initialize, .nm_modname = "vesper_player_harmony_node",
    .nm_priv = nullptr, .reserved = {nullptr},
};
extern "C" __attribute__((constructor)) void RegisterVesperHarmonyModule()
{
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    napi_module_register(&module);
#pragma clang diagnostic pop
}
#endif
