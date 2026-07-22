#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/common.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
PROFILE="debug"
MODE="swift-smoke"
SOURCE_PATH_OVERRIDE="${VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE:-}"
STAGING_DIR_OVERRIDE="${VESPER_IOS_NATIVE_FRAME_STAGING_DIR:-}"
FFMPEG_APPLE_BASE_DIR="$ROOT_DIR/third_party/ffmpeg/apple"
RUNTIME_PROFILE="${VESPER_IOS_SOURCE_NORMALIZER_RUNTIME_PROFILE:-}"

usage() {
  cat <<EOF >&2
Usage: $(basename "$0") [debug|release] [swift-smoke]

Examples:
  $(basename "$0")
  $(basename "$0") debug swift-smoke
EOF
}

for token in "$@"; do
  case "$token" in
    debug|release)
      PROFILE="$token"
      ;;
    swift-smoke)
      MODE="$token"
      ;;
    *)
      usage
      exit 1
      ;;
  esac
done

resolve_smoke_source() {
  local target_dir="$1"
  local generated="$target_dir/ios-native-frame-smoke-h264-aac.mp4"

  if [[ -n "$SOURCE_PATH_OVERRIDE" ]]; then
    if [[ ! -f "$SOURCE_PATH_OVERRIDE" ]]; then
      echo "VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE points to a missing file: $SOURCE_PATH_OVERRIDE" >&2
      exit 1
    fi
    printf '%s\n' "$SOURCE_PATH_OVERRIDE"
    return 0
  fi

  if [[ -f "$generated" ]]; then
    printf '%s\n' "$generated"
    return 0
  fi

  if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "ffmpeg is required to generate the iOS native-frame smoke source; install ffmpeg or set VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE." >&2
    exit 1
  fi

  mkdir -p "$target_dir"
  ffmpeg \
    -hide_banner \
    -loglevel error \
    -y \
    -f lavfi \
    -i testsrc2=size=320x180:rate=24:duration=3 \
    -f lavfi \
    -i sine=frequency=440:sample_rate=48000:duration=3 \
    -c:v libx264 \
    -profile:v baseline \
    -level:v 3.1 \
    -pix_fmt yuv420p \
    -c:a aac \
    -b:a 96k \
    -shortest \
    -movflags +faststart \
    "$generated"
  printf '%s\n' "$generated"
}

ensure_loader_rpath() {
  local binary_path="$1"
  local rpath="$2"

  if ! otool -l "$binary_path" | grep -Fq "$rpath"; then
    install_name_tool -add_rpath "$rpath" "$binary_path"
  fi
}

prepare_staged_dylib() {
  local source_path="$1"
  local output_path="$2"

  cp "$source_path" "$output_path"
  install_name_tool -id "@rpath/$(basename "$output_path")" "$output_path"
  ensure_loader_rpath "$output_path" "@loader_path"
}

copy_ffmpeg_runtime_dylibs() {
  local ffmpeg_lib_dir="$1"
  local staging_dir="$2"
  local copied_count=0
  local runtime_binary

  while IFS= read -r runtime_binary; do
    if [[ -L "$runtime_binary" ]]; then
      cp "$(realpath "$runtime_binary")" "$staging_dir/$(basename "$runtime_binary")"
    else
      cp "$runtime_binary" "$staging_dir/"
    fi
    copied_count=$((copied_count + 1))
  done < <(find "$ffmpeg_lib_dir" -maxdepth 1 \( -type f -o -type l \) -name 'lib*.dylib*' | sort)

  if [[ "$copied_count" -eq 0 ]]; then
    echo "Missing FFmpeg runtime dylibs in: $ffmpeg_lib_dir" >&2
    exit 1
  fi

  while IFS= read -r runtime_binary; do
    install_name_tool -id "@rpath/$(basename "$runtime_binary")" "$runtime_binary"
    ensure_loader_rpath "$runtime_binary" "@loader_path"
  done < <(find "$staging_dir" -maxdepth 1 -type f -name 'lib*.dylib*' | sort)
}

build_ios_plugins() {
  local staging_dir="$1"
  local build_root="$ROOT_DIR/target/ios-native-frame-smoke"
  local source_output_dir="$build_root/player-source-normalizer-ffmpeg"
  local decoder_output_dir="$build_root/player-decoder-videotoolbox"
  local frame_processor_output_dir="$build_root/player-frame-processor-diagnostic"
  local ffmpeg_apple_dir
  local ffmpeg_dir
  local ffmpeg_libdir

  vesper_ffmpeg_parse_common_args apple
  ffmpeg_apple_dir="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir apple "$FFMPEG_APPLE_BASE_DIR")}}"
  ffmpeg_dir="$(vesper_apple_slice_output_root ios-simulator-arm64 "$ffmpeg_apple_dir")"
  ffmpeg_libdir="$(vesper_apple_slice_output_libdir ios-simulator-arm64)"
  if [[ ! -d "$ffmpeg_dir/lib/$ffmpeg_libdir" ]]; then
    echo "Missing Apple FFmpeg simulator runtime: $ffmpeg_dir/lib/$ffmpeg_libdir" >&2
    echo "Run ./scripts/ffmpeg/build.sh --platform ios or seed third_party/ffmpeg/apple locally." >&2
    exit 1
  fi

  env \
    VESPER_SKIP_APPLE_FFMPEG_PREBUILDS=1 \
    "$ROOT_DIR/scripts/ios/build-player-plugin.sh" \
      source-normalizer-ffmpeg \
      "$source_output_dir" \
      "$PROFILE" \
      ios-simulator-arm64
  "$ROOT_DIR/scripts/ios/build-player-plugin.sh" \
    decoder-videotoolbox \
    "$decoder_output_dir" \
    "$PROFILE" \
    ios-simulator-arm64
  "$ROOT_DIR/scripts/ios/build-player-plugin.sh" \
    frame-processor-diagnostic \
    "$frame_processor_output_dir" \
    "$PROFILE" \
    ios-simulator-arm64

  copy_ffmpeg_runtime_dylibs "$ffmpeg_dir/lib/$ffmpeg_libdir" "$staging_dir"
  prepare_staged_dylib \
    "$source_output_dir/iphonesimulator/libvesper_source_normalizer_ffmpeg.dylib" \
    "$staging_dir/libvesper_source_normalizer_ffmpeg.dylib"
  prepare_staged_dylib \
    "$decoder_output_dir/iphonesimulator/libvesper_decoder_videotoolbox.dylib" \
    "$staging_dir/libvesper_decoder_videotoolbox.dylib"
  prepare_staged_dylib \
    "$frame_processor_output_dir/iphonesimulator/libvesper_frame_processor_diagnostic.dylib" \
    "$staging_dir/libvesper_frame_processor_diagnostic.dylib"
}

run_swift_smoke_test() {
  local staging_dir="$1"
  local source_path="$2"
  local config_path="/tmp/vesper-ios-native-frame-smoke.plist"
  local log_file
  local status=0

  rm -f "$config_path"
  /usr/bin/plutil -create xml1 "$config_path"
  /usr/libexec/PlistBuddy -c "Add :VESPER_IOS_NATIVE_FRAME_SMOKE_ENABLED string 1" "$config_path"
  /usr/libexec/PlistBuddy -c "Add :VESPER_IOS_NATIVE_FRAME_SMOKE_SOURCE string $source_path" "$config_path"
  /usr/libexec/PlistBuddy -c "Add :VESPER_IOS_SOURCE_NORMALIZER_PLUGIN_PATH string $staging_dir/libvesper_source_normalizer_ffmpeg.dylib" "$config_path"
  /usr/libexec/PlistBuddy -c "Add :VESPER_IOS_DECODER_VIDEOTOOLBOX_PLUGIN_PATH string $staging_dir/libvesper_decoder_videotoolbox.dylib" "$config_path"
  /usr/libexec/PlistBuddy -c "Add :VESPER_IOS_FRAME_PROCESSOR_DIAGNOSTIC_PLUGIN_PATH string $staging_dir/libvesper_frame_processor_diagnostic.dylib" "$config_path"
  /usr/libexec/PlistBuddy -c "Add :VESPER_IOS_SOURCE_NORMALIZER_RUNTIME_PROFILE string $RUNTIME_PROFILE" "$config_path"

  log_file="$(mktemp "${TMPDIR:-/tmp}/vesper-ios-native-frame-smoke.XXXXXX").log"
  echo "Running iOS native-frame Swift smoke; log: $log_file"

  set +e
  env \
    VESPER_FRAME_PROCESSOR_DIAGNOSTIC_MODE=noop \
    xcodebuild test \
      -project "$PROJECT_DIR/VesperPlayerKit.xcodeproj" \
      -scheme VesperPlayerKit \
      -destination "platform=iOS Simulator,name=iPhone 17,OS=26.5" \
      CODE_SIGNING_ALLOWED=NO \
      CODE_SIGNING_REQUIRED=NO \
      -only-testing:VesperPlayerKitTests/VesperPlayerControllerStateTests/testNativeFramePipelineRealPluginPlaybackPresentsSeeksAndReleasesLocalMp4 \
      2>&1 | tee "$log_file"
  status=${PIPESTATUS[0]}
  set -e

  if [[ "$status" -ne 0 ]]; then
    exit "$status"
  fi
  if grep -Fq "native-frame release failed" "$log_file"; then
    echo "iOS native-frame smoke reported a frame release failure." >&2
    echo "Log: $log_file" >&2
    grep -F "native-frame release failed" "$log_file" >&2 || true
    exit 1
  fi
  if grep -Fq "invalid iOS native-frame pending frame handle" "$log_file"; then
    echo "iOS native-frame smoke reported an invalid pending-frame handle." >&2
    echo "Log: $log_file" >&2
    grep -F "invalid iOS native-frame pending frame handle" "$log_file" >&2 || true
    exit 1
  fi
  if ! grep -Fq "real iOS native-frame smoke presentedFrames=" "$log_file"; then
    echo "iOS native-frame smoke did not report its real playback summary." >&2
    echo "Log: $log_file" >&2
    tail -n 80 "$log_file" >&2 || true
    exit 1
  fi

  echo "iOS native-frame Swift smoke passed; log: $log_file"
}

main() {
  local staging_dir
  local source_path

  if [[ "$MODE" != "swift-smoke" ]]; then
    usage
    exit 1
  fi
  if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "iOS native-frame pipeline verification only runs on macOS." >&2
    exit 1
  fi

  if [[ -n "$STAGING_DIR_OVERRIDE" ]]; then
    staging_dir="$STAGING_DIR_OVERRIDE"
    rm -rf "$staging_dir"
    mkdir -p "$staging_dir"
  else
    staging_dir="$(mktemp -d "${TMPDIR:-/tmp}/vesper-ios-native-frame-smoke.XXXXXX")"
  fi

  source_path="$(resolve_smoke_source "$ROOT_DIR/target")"
  build_ios_plugins "$staging_dir"

  echo "Using iOS native-frame smoke source: $source_path"
  echo "Using staged iOS native-frame plugins: $staging_dir"
  run_swift_smoke_test "$staging_dir" "$source_path"
}

main
