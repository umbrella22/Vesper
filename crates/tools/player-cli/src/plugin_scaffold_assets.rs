//! Compile-time assets for the standalone `vesper` binary.

pub(crate) const RUST_CARGO_TEMPLATE: &str = r###"[package]
name = {{CRATE_NAME}}
version = "0.1.0"
edition = "2024"
rust-version = "{{RUST_VERSION}}"
license = {{LICENSE}}

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
player-plugin = { package = "vesper-player-plugin", version = "={{VESPER_SDK_VERSION}}" }

[workspace]
"###;

pub(crate) const RUST_SOURCE_TEMPLATE: &str = r###"#![deny(unsafe_code)]

use player_plugin::author::{
    AudioBlock, AudioProcessor, AudioProcessorCapabilities, AudioProcessorFactory,
    OpenAudioProcessor, Plugin, PluginError, PluginResult, VesperAudioFormat,
};

const MODES: AudioProcessorCapabilities = AudioProcessorCapabilities {
    fixed_block: true,
    buffered: false,
};
const GAIN: f32 = 0.5;

struct GainFactory;
struct Gain;

impl AudioProcessorFactory for GainFactory {
    fn capabilities(&self) -> AudioProcessorCapabilities {
        MODES
    }

    fn capabilities_json(&self) -> &str {
        r#"{"fixedBlock":true}"#
    }

    fn open(&self, input: VesperAudioFormat, _params: &[u8]) -> PluginResult<OpenAudioProcessor> {
        Ok(OpenAudioProcessor {
            output_format: input,
            processor: Box::new(Gain),
        })
    }
}

impl AudioProcessor for Gain {
    fn capabilities(&self) -> AudioProcessorCapabilities {
        MODES
    }

    fn process_block(&mut self, block: AudioBlock<'_>) -> PluginResult<()> {
        match block {
            AudioBlock::InPlace { mut audio, .. } => {
                for sample in audio.samples() {
                    *sample *= GAIN;
                }
            }
            AudioBlock::Split { input, mut output } => {
                for (target, &sample) in output.samples().iter_mut().zip(input.samples()) {
                    *target = sample * GAIN;
                }
            }
        }
        Ok(())
    }

    fn set_playback_policy(&mut self, _rate: f32, _pitch: u32, _epoch: u64) -> PluginResult<()> {
        Ok(())
    }

    fn set_param(&mut self, _key: &[u8], _value: &[u8]) -> PluginResult<()> {
        Err(PluginError::Contract("this gain processor has no parameters"))
    }

    fn latency_frames(&self) -> u32 {
        0
    }

    fn flush(&mut self) -> PluginResult<()> {
        Ok(())
    }
}

#[player_plugin::author::export]
pub fn gain_plugin() -> Plugin {
    Plugin::new({{PLUGIN_ID}}, {{PLUGIN_NAME}}).with_audio_processor(GainFactory)
}
"###;

pub(crate) const RUST_README: &str = r###"# Rust native gain plugin

The factory exposes one fixed-block `AudioProcessor` through both the static Rust author API and the `vesper_native_plugin_entry` symbol. Each call halves the input samples and leaves the negotiated sample rate, channel count, and frame count unchanged.

Copy `Cargo.toml.template` to `Cargo.toml` and `src/lib.rs.template` to `src/lib.rs`. Replace `{{VESPER_SDK_VERSION}}` with an SDK version that exports native plugin ABI. For a source-checkout build before publication, specify the local `crates/plugin/player-plugin` path for that dependency. Run `cargo build --release` to produce the native library. Linked hosts can call `gain_plugin()` directly.
"###;

pub(crate) const CPP_CMAKE: &str = r###"cmake_minimum_required(VERSION 3.16)
project(vesper_gain_cpp LANGUAGES CXX)

set(VESPER_PLUGIN_HEADERS "" CACHE PATH "Directory containing vesper_plugin.h")
if(NOT EXISTS "${VESPER_PLUGIN_HEADERS}/vesper_plugin.h")
    message(FATAL_ERROR "Set VESPER_PLUGIN_HEADERS to the directory containing the native plugin ABI header")
endif()

add_library(vesper_gain_cpp SHARED gain.cpp)
target_include_directories(vesper_gain_cpp PRIVATE "${VESPER_PLUGIN_HEADERS}")
target_compile_definitions(vesper_gain_cpp PRIVATE VESPER_PLUGIN_BUILD)
target_compile_features(vesper_gain_cpp PRIVATE cxx_std_17)
set_target_properties(vesper_gain_cpp PROPERTIES CXX_VISIBILITY_PRESET hidden CXX_EXTENSIONS OFF)
"###;

pub(crate) const CPP_SOURCE: &str = r###"#include "vesper_plugin.h"

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
"###;

pub(crate) const CPP_README: &str = r###"# C++17 native gain plugin

The shared library exports `vesper_native_plugin_entry` and one fixed-block `AudioProcessor`. Split processing reads the input and writes the output. In-place processing uses the same writable buffer descriptor for both arguments. Both modes preserve the time and rate epoch of the input block.

```sh
cmake -S . -B build -DVESPER_PLUGIN_HEADERS=/path/to/sdk/include
cmake --build build --config Release
```

The build needs the native plugin ABI header and a C++17 compiler. It has no Rust crate dependencies.
"###;

pub(crate) const APACHE_2_LICENSE: &str = r#"                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright 2026 umbrella22

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
"#;
