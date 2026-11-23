#!/usr/bin/env bash
set -euo pipefail

probe_scripts="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
probe_repo="$(cd "$probe_scripts/../.." && pwd)"
cd "$probe_repo"

if [[ "$(uname -s)" != Darwin ]]; then
  echo 'The native plugin probe requires macOS and a Metal device.' >&2
  exit 1
fi
for tool in cargo cbindgen ffmpeg python3 xcrun; do
  command -v "$tool" >/dev/null || { echo "Missing $tool" >&2; exit 1; }
done

mkdir -p "$probe_repo/target"
probe_run="$(mktemp -d "$probe_repo/target/native-plugin-probe.XXXXXX")"
echo "Probe artifacts: $probe_run"
abi_source=crates/platform/desktop/player-platform-macos/examples/native_plugin_probe/abi.rs
mkdir "$probe_run/external"
cbindgen --config "$probe_scripts/cbindgen.toml" --output "$probe_run/external/plugin_probe_abi.h" "$abi_source"
cp "$probe_scripts/gain.cpp" "$probe_scripts/filter.h" "$probe_scripts/filter.mm" \
  "$probe_scripts/metal.mm" "$probe_run/external/"
(
  cd "$probe_run/external"
  xcrun clang -x c -std=c99 -Wall -Wextra -Werror -fsyntax-only filter.h
  xcrun clang++ -O2 -std=c++17 -Wall -Wextra -Werror -dynamiclib gain.cpp -o libgain.dylib
  xcrun clang++ -O2 -std=c++17 -fobjc-arc -Wall -Wextra -Werror -dynamiclib filter.mm \
    -framework Foundation -framework Metal -install_name @rpath/libprobe_filter.dylib -o libprobe_filter.dylib
  xcrun clang++ -O2 -std=c++17 -fobjc-arc -Wall -Wextra -Werror -dynamiclib metal.mm \
    -L. -lprobe_filter -Wl,-rpath,@loader_path \
    -framework Foundation -framework CoreVideo -framework Metal -framework AVFoundation -o libmetal_probe.dylib
)

# Keep experiment binaries out of the release staging directory.
probe_build="$probe_repo/target/native-plugin-probe-build"
export CARGO_TARGET_DIR="$probe_build"
(
  cargo build --locked --release -p player-decoder-videotoolbox -p player-source-normalizer-ffmpeg
  cargo build --locked --release -p player-platform-macos --example native_plugin_probe
  cargo test --locked -p player-platform-macos --example native_plugin_probe
  cargo test --locked -p player-audio-cpal drained_rate_segments_keep_media_position_continuous_before_seek
  cargo test --locked -p player-platform-native-frame leased_packet_survives_decoder_backpressure_until_seek_or_close
) >"$probe_run/build.log" 2>&1 || { tail -60 "$probe_run/build.log" >&2; exit 1; }

for size in 1920x1080 3840x2160; do
  fixture="$probe_run/$size.mp4"
  ffmpeg -nostdin -v error -f lavfi -i "testsrc2=size=$size:rate=60" \
    -f lavfi -i 'sine=frequency=431:sample_rate=48000' -t 4 \
    -c:v libx264 -preset ultrafast -crf 25 -pix_fmt yuv420p -g 60 -bf 0 -threads:v 4 \
    -c:a aac -ac 2 "$fixture"

  audio_args=()
  if [[ "${VESPER_RUN_PHYSICAL_AUDIO_TESTS:-}" == 1 ]]; then
    pcm="$probe_run/$size.f32le"
    ffmpeg -nostdin -v error -i "$fixture" -map 0:a:0 -ac 2 -ar 48000 -c:a pcm_f32le -f f32le "$pcm"
    audio_args=("$pcm")
  fi

  python3 - \
    "$probe_build/release/examples/native_plugin_probe" "$probe_run/external/libgain.dylib" \
    "$probe_run/external/libmetal_probe.dylib" "$probe_build/release" "$fixture" ${audio_args[@]+"${audio_args[@]}"} <<'PY' | tee -a "$probe_run/results.log"
import subprocess
import sys

try:
    subprocess.run(sys.argv[1:], timeout=90, check=True)
except subprocess.TimeoutExpired as error:
    sys.exit(f"Native plugin probe exceeded {error.timeout} seconds")
except subprocess.CalledProcessError as error:
    sys.exit(f"Native plugin probe exited with status {error.returncode}")
PY
done
