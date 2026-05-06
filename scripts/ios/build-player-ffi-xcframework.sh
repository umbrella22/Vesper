#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/ios/VesperPlayerKit"
OUTPUT_DIR="$PROJECT_DIR/Artifacts/rust-player-ffi"
XCFRAMEWORK_PATH="$OUTPUT_DIR/VesperPlayerFFI.xcframework"
HEADERS_DIR="$PROJECT_DIR/Sources/VesperPlayerFFIResolver/include"
PROFILE="${1:-release}"
BUILD_MODE="${VESPER_BUILD_IOS_PLAYER_FFI_MODE:-full}"
PLATFORM_FILTER="${PLATFORM_NAME:-}"

vesper_require_rust_tools_for_xcode

if [[ "$PROFILE" != "debug" && "$PROFILE" != "release" ]]; then
  echo "Unsupported profile: $PROFILE" >&2
  echo "Usage: $0 [debug|release]" >&2
  exit 1
fi

if [[ "$BUILD_MODE" != "full" && "$BUILD_MODE" != "platform" ]]; then
  echo "Unsupported build mode: $BUILD_MODE" >&2
  echo "Supported modes: full, platform" >&2
  exit 1
fi

PROFILE_DIR="$PROFILE"
BUILD_FLAGS=()
if [[ "$PROFILE" == "release" ]]; then
  BUILD_FLAGS+=(--release)
fi

# Apple binary distribution is arm64-only; do not reintroduce x86_64
# iOS Simulator or Mac Catalyst slices.
DEVICE_TARGET="aarch64-apple-ios"
SIMULATOR_TARGETS=(
  "aarch64-apple-ios-sim"
)
CATALYST_TARGETS=(
  "aarch64-apple-ios-macabi"
)
installed_targets="$(rustup target list --installed)"

vesper_apple_require_rust_targets "$DEVICE_TARGET" "${SIMULATOR_TARGETS[@]}"

target_is_installed() {
  local target="$1"
  [[ "$installed_targets" == *"$target"* ]]
}

resolve_optional_targets() {
  local target
  for target in "$@"; do
    if target_is_installed "$target"; then
      printf '%s\n' "$target"
    fi
  done
}

build_target() {
  local target="$1"
  local build_command=(cargo build --target "$target" -p player-ffi-ios)
  if [[ "$PROFILE" == "release" ]]; then
    build_command+=(--release)
  fi
  "${build_command[@]}"
}

copy_built_library() {
  local source_path="$1"
  local destination_path="$2"
  mkdir -p "$(dirname "$destination_path")"
  cp "$source_path" "$destination_path"
}

strip_static_archive_if_needed() {
  local archive_path="$1"

  if [[ "$PROFILE" != "release" ]]; then
    return 0
  fi

  xcrun strip -S -x "$archive_path"
}

build_device_archive() {
  build_target "$DEVICE_TARGET"
  copy_built_library \
    "$ROOT_DIR/target/$DEVICE_TARGET/$PROFILE_DIR/libplayer_ffi_ios.a" \
    "$OUTPUT_DIR/iphoneos/libplayer_ffi_ios.a"
  strip_static_archive_if_needed "$OUTPUT_DIR/iphoneos/libplayer_ffi_ios.a"
}

build_simulator_archive() {
  local simulator_archives=()
  local target
  for target in "${SIMULATOR_TARGETS[@]}"; do
    build_target "$target"

    local simulator_output_dir="$OUTPUT_DIR/$target"
    local simulator_output_path="$simulator_output_dir/libplayer_ffi_ios.a"
    copy_built_library \
      "$ROOT_DIR/target/$target/$PROFILE_DIR/libplayer_ffi_ios.a" \
      "$simulator_output_path"
    strip_static_archive_if_needed "$simulator_output_path"
    simulator_archives+=("$simulator_output_path")
  done

  mkdir -p "$OUTPUT_DIR/iphonesimulator"
  if [[ ${#simulator_archives[@]} -eq 1 ]]; then
    cp "${simulator_archives[0]}" "$OUTPUT_DIR/iphonesimulator/libplayer_ffi_ios.a"
  else
    lipo -create "${simulator_archives[@]}" \
      -output "$OUTPUT_DIR/iphonesimulator/libplayer_ffi_ios.a"
  fi
  strip_static_archive_if_needed "$OUTPUT_DIR/iphonesimulator/libplayer_ffi_ios.a"
}

build_catalyst_archive() {
  local resolved_catalyst_targets=()
  while IFS= read -r target; do
    if [[ -n "$target" ]]; then
      resolved_catalyst_targets+=("$target")
    fi
  done < <(resolve_optional_targets "${CATALYST_TARGETS[@]}")

  if [[ ${#resolved_catalyst_targets[@]} -eq 0 ]]; then
    echo "Missing Rust Apple target for Mac Catalyst." >&2
    echo "Install at least one of: ${CATALYST_TARGETS[*]}" >&2
    exit 1
  fi

  local catalyst_archives=()
  local target
  for target in "${resolved_catalyst_targets[@]}"; do
    build_target "$target"

    local catalyst_output_dir="$OUTPUT_DIR/$target"
    local catalyst_output_path="$catalyst_output_dir/libplayer_ffi_ios.a"
    copy_built_library \
      "$ROOT_DIR/target/$target/$PROFILE_DIR/libplayer_ffi_ios.a" \
      "$catalyst_output_path"
    strip_static_archive_if_needed "$catalyst_output_path"
    catalyst_archives+=("$catalyst_output_path")
  done

  mkdir -p "$OUTPUT_DIR/macosx"
  if [[ ${#catalyst_archives[@]} -eq 1 ]]; then
    cp "${catalyst_archives[0]}" "$OUTPUT_DIR/macosx/libplayer_ffi_ios.a"
  else
    lipo -create "${catalyst_archives[@]}" \
      -output "$OUTPUT_DIR/macosx/libplayer_ffi_ios.a"
  fi
  strip_static_archive_if_needed "$OUTPUT_DIR/macosx/libplayer_ffi_ios.a"
}

if [[ "$BUILD_MODE" == "platform" ]]; then
  mkdir -p "$OUTPUT_DIR"
  case "$PLATFORM_FILTER" in
    iphoneos)
      rm -rf "$OUTPUT_DIR/iphoneos"
      build_device_archive
      ;;
    iphonesimulator)
      rm -rf "$OUTPUT_DIR/iphonesimulator"
      for target in "${SIMULATOR_TARGETS[@]}"; do
        rm -rf "$OUTPUT_DIR/$target"
      done
      build_simulator_archive
      ;;
    macosx)
      rm -rf "$OUTPUT_DIR/macosx"
      for target in "${CATALYST_TARGETS[@]}"; do
        rm -rf "$OUTPUT_DIR/$target"
      done
      build_catalyst_archive
      ;;
    *)
      echo "Unsupported or missing PLATFORM_NAME for platform mode: ${PLATFORM_FILTER:-<empty>}" >&2
      echo "Expected one of: iphoneos, iphonesimulator, macosx" >&2
      exit 1
      ;;
  esac

  echo
  echo "Built player-ffi Apple artifacts into:"
  echo "  $OUTPUT_DIR"
  exit 0
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

build_device_archive

build_simulator_archive

resolved_catalyst_targets=()
while IFS= read -r target; do
  if [[ -n "$target" ]]; then
    resolved_catalyst_targets+=("$target")
  fi
done < <(resolve_optional_targets "${CATALYST_TARGETS[@]}")

should_build_catalyst=false
if [[ "${PLATFORM_NAME:-}" == "macosx" || "${VESPER_BUILD_APPLE_CATALYST:-0}" == "1" ]]; then
  should_build_catalyst=true
  if [[ ${#resolved_catalyst_targets[@]} -eq 0 ]]; then
    echo "Missing Rust Apple target for Mac Catalyst." >&2
    echo "Install at least one of: ${CATALYST_TARGETS[*]}" >&2
    exit 1
  fi
elif [[ ${#resolved_catalyst_targets[@]} -gt 0 ]]; then
  should_build_catalyst=true
fi

if [[ "$should_build_catalyst" == "true" ]]; then
  build_catalyst_archive
fi

rm -rf "$XCFRAMEWORK_PATH"
xcframework_command=(
  xcodebuild -create-xcframework
  -library "$OUTPUT_DIR/iphoneos/libplayer_ffi_ios.a"
  -headers "$HEADERS_DIR"
  -library "$OUTPUT_DIR/iphonesimulator/libplayer_ffi_ios.a"
  -headers "$HEADERS_DIR"
)

if [[ "$should_build_catalyst" == "true" ]]; then
  xcframework_command+=(
    -library "$OUTPUT_DIR/macosx/libplayer_ffi_ios.a"
    -headers "$HEADERS_DIR"
  )
fi

xcframework_command+=(
  -output "$XCFRAMEWORK_PATH"
)

"${xcframework_command[@]}"

echo
echo "Built player-ffi Apple artifacts into:"
echo "  $OUTPUT_DIR"
