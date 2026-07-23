#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/android"
CORE_MODULE_DIR="$PROJECT_DIR/vesper-player-kit"
COMPOSE_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-compose"
COMPOSE_UI_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-compose-ui"
EXTERNAL_PLAYBACK_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-external-playback"
FFMPEG_RUNTIME_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-ffmpeg-runtime"
DECODER_MEDIACODEC_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-decoder-mediacodec"
SOURCE_NORMALIZER_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-source-normalizer-ffmpeg"
FRAME_PROCESSOR_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-frame-processor-diagnostic"
FALLBACK_PROJECT_DIR="$ROOT_DIR/examples/android-compose-host"
OUTPUT_DIR="${1:-$ROOT_DIR/dist/release/android}"
shift || true

selected_abis=("$@")
if [[ ${#selected_abis[@]} -eq 0 ]]; then
  selected_abis=("${VESPER_ANDROID_DEFAULT_ABIS[@]}")
fi

include_optional_plugins=0
case "${VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS:-0}" in
  1|true|TRUE|yes|YES)
    include_optional_plugins=1
    ;;
esac

if [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
  cat >"$PROJECT_DIR/local.properties" <<EOF
sdk.dir=${ANDROID_SDK_ROOT}
EOF
fi

GRADLE_CMD=("$(vesper_android_resolve_gradle "$PROJECT_DIR" "$FALLBACK_PROJECT_DIR")" -p "$PROJECT_DIR")

mkdir -p "$OUTPUT_DIR"

for abi in "${selected_abis[@]}"; do
  case "$abi" in
    arm64-v8a)
      ;;
    *)
      echo "Unsupported Android ABI: $abi" >&2
      exit 1
      ;;
  esac

  if [[ "$include_optional_plugins" -eq 0 ]]; then
    rm -f \
      "$OUTPUT_DIR/VesperPlayerKitExternalPlayback-android-$abi.aar" \
      "$OUTPUT_DIR/VesperPlayerKitFfmpegRuntime-android-$abi.aar" \
      "$OUTPUT_DIR/VesperPlayerKitDecoderMediaCodec-android-$abi.aar" \
      "$OUTPUT_DIR/VesperPlayerKitSourceNormalizerFfmpeg-android-$abi.aar" \
      "$OUTPUT_DIR/VesperPlayerKitFrameProcessorDiagnostic-android-$abi.aar"
  fi

  rm -rf "$CORE_MODULE_DIR/src/main/jniLibs"
  if [[ "$include_optional_plugins" -eq 1 ]]; then
    "$ROOT_DIR/scripts/vesper" ffmpeg --platform android --profile default --abi "$abi"
  fi

  "${GRADLE_CMD[@]}" \
    :vesper-player-kit:clean \
    :vesper-player-kit-compose:clean \
    :vesper-player-kit-compose-ui:clean

  if [[ "$include_optional_plugins" -eq 1 ]]; then
    "${GRADLE_CMD[@]}" \
      :vesper-player-kit-external-playback:clean \
      :vesper-player-kit-ffmpeg-runtime:clean \
      :vesper-player-kit-decoder-mediacodec:clean \
      :vesper-player-kit-source-normalizer-ffmpeg:clean \
      :vesper-player-kit-frame-processor-diagnostic:clean
  fi

  if [[ "$include_optional_plugins" -eq 1 ]]; then
    RUST_ANDROID_ABIS="$abi" \
      "$ROOT_DIR/scripts/android/build-player-decoder-mediacodec-plugin.sh" \
        "$DECODER_MEDIACODEC_MODULE_DIR/src/main/jniLibs" \
        release
    RUST_ANDROID_ABIS="$abi" \
      "$ROOT_DIR/scripts/android/build-player-source-normalizer-ffmpeg-plugin.sh" \
        "$SOURCE_NORMALIZER_MODULE_DIR/src/main/jniLibs" \
        release \
        --profile default \
        --metadata-dir "$SOURCE_NORMALIZER_MODULE_DIR/src/main/assets/vesper-source-normalizer-ffmpeg"
    RUST_ANDROID_ABIS="$abi" \
      "$ROOT_DIR/scripts/android/build-player-frame-processor-diagnostic-plugin.sh" \
        "$FRAME_PROCESSOR_MODULE_DIR/src/main/jniLibs" \
        release
  fi

  if [[ "$include_optional_plugins" -eq 1 ]]; then
    RUST_ANDROID_ABIS="$abi" \
    VESPER_ANDROID_SKIP_FFMPEG_RUNTIME_BUILD=1 \
      "${GRADLE_CMD[@]}" \
      -Pvesper.player.android.abis="$abi" \
      -Pvesper.player.android.external.nativeBuildProfile=release \
      -Pvesper.player.android.external.ffmpegProfile=default \
      :vesper-player-kit-external-playback:buildRelayFfmpegAndroidJni
  fi
  assemble_tasks=(
    :vesper-player-kit:assembleRelease
    :vesper-player-kit-compose:assembleRelease
    :vesper-player-kit-compose-ui:assembleRelease
  )
  if [[ "$include_optional_plugins" -eq 1 ]]; then
    assemble_tasks+=(
      :vesper-player-kit-external-playback:assembleRelease
      :vesper-player-kit-ffmpeg-runtime:assembleRelease
      :vesper-player-kit-decoder-mediacodec:assembleRelease
      :vesper-player-kit-source-normalizer-ffmpeg:assembleRelease
      :vesper-player-kit-frame-processor-diagnostic:assembleRelease
    )
  fi

  RUST_ANDROID_ABIS="$abi" \
  VESPER_ANDROID_SKIP_FFMPEG_RUNTIME_BUILD=1 \
    "${GRADLE_CMD[@]}" \
    -Pvesper.player.android.abis="$abi" \
    -Pvesper.player.android.external.nativeBuildProfile=release \
    -Pvesper.player.android.external.ffmpegProfile=default \
    "${assemble_tasks[@]}"

  CORE_INPUT_AAR="$CORE_MODULE_DIR/build/outputs/aar/vesper-player-kit-release.aar"
  CORE_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKit-android-$abi.aar"
  cp "$CORE_INPUT_AAR" "$CORE_OUTPUT_AAR"

  COMPOSE_INPUT_AAR="$COMPOSE_MODULE_DIR/build/outputs/aar/vesper-player-kit-compose-release.aar"
  COMPOSE_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitCompose-android-$abi.aar"
  cp "$COMPOSE_INPUT_AAR" "$COMPOSE_OUTPUT_AAR"

  COMPOSE_UI_INPUT_AAR="$COMPOSE_UI_MODULE_DIR/build/outputs/aar/vesper-player-kit-compose-ui-release.aar"
  COMPOSE_UI_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitComposeUi-android-$abi.aar"
  cp "$COMPOSE_UI_INPUT_AAR" "$COMPOSE_UI_OUTPUT_AAR"

  if [[ "$include_optional_plugins" -eq 1 ]]; then
    EXTERNAL_PLAYBACK_INPUT_AAR="$EXTERNAL_PLAYBACK_MODULE_DIR/build/outputs/aar/vesper-player-kit-external-playback-release.aar"
    EXTERNAL_PLAYBACK_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitExternalPlayback-android-$abi.aar"
    cp "$EXTERNAL_PLAYBACK_INPUT_AAR" "$EXTERNAL_PLAYBACK_OUTPUT_AAR"

    FFMPEG_RUNTIME_INPUT_AAR="$FFMPEG_RUNTIME_MODULE_DIR/build/outputs/aar/vesper-player-kit-ffmpeg-runtime-release.aar"
    FFMPEG_RUNTIME_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitFfmpegRuntime-android-$abi.aar"
    cp "$FFMPEG_RUNTIME_INPUT_AAR" "$FFMPEG_RUNTIME_OUTPUT_AAR"

    DECODER_MEDIACODEC_INPUT_AAR="$DECODER_MEDIACODEC_MODULE_DIR/build/outputs/aar/vesper-player-kit-decoder-mediacodec-release.aar"
    DECODER_MEDIACODEC_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitDecoderMediaCodec-android-$abi.aar"
    cp "$DECODER_MEDIACODEC_INPUT_AAR" "$DECODER_MEDIACODEC_OUTPUT_AAR"
    decoder_mediacodec_entries="$(unzip -Z1 "$DECODER_MEDIACODEC_OUTPUT_AAR")"
    if ! grep -q '^jni/'"$abi"'/libvesper_decoder_mediacodec\.so$' <<<"$decoder_mediacodec_entries"; then
      echo "Android MediaCodec decoder AAR is missing libvesper_decoder_mediacodec.so:" >&2
      echo "  $DECODER_MEDIACODEC_OUTPUT_AAR" >&2
      exit 1
    fi
    if grep -E '(^|/)jni/[^/]+/(libav|libsw|libxml2|libssl|libcrypto).*\.so$' <<<"$decoder_mediacodec_entries" >/dev/null; then
      echo "Android MediaCodec decoder AAR must not bundle FFmpeg runtime libraries:" >&2
      grep -E '(^|/)jni/[^/]+/(libav|libsw|libxml2|libssl|libcrypto).*\.so$' <<<"$decoder_mediacodec_entries" >&2
      exit 1
    fi

    SOURCE_NORMALIZER_INPUT_AAR="$SOURCE_NORMALIZER_MODULE_DIR/build/outputs/aar/vesper-player-kit-source-normalizer-ffmpeg-release.aar"
    SOURCE_NORMALIZER_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitSourceNormalizerFfmpeg-android-$abi.aar"
    cp "$SOURCE_NORMALIZER_INPUT_AAR" "$SOURCE_NORMALIZER_OUTPUT_AAR"
    source_normalizer_entries="$(unzip -Z1 "$SOURCE_NORMALIZER_OUTPUT_AAR")"
    if ! grep -q '^assets/vesper-source-normalizer-ffmpeg/profile-hash.txt$' <<<"$source_normalizer_entries"; then
      echo "Android SourceNormalizer AAR is missing profile-hash.txt metadata:" >&2
      echo "  $SOURCE_NORMALIZER_OUTPUT_AAR" >&2
      exit 1
    fi
    if grep -E '(^|/)jni/[^/]+/(libav|libsw|libxml2|libssl|libcrypto).*\.so$' <<<"$source_normalizer_entries" >/dev/null; then
      echo "Android SourceNormalizer AAR must not bundle FFmpeg runtime libraries:" >&2
      grep -E '(^|/)jni/[^/]+/(libav|libsw|libxml2|libssl|libcrypto).*\.so$' <<<"$source_normalizer_entries" >&2
      exit 1
    fi

    FRAME_PROCESSOR_INPUT_AAR="$FRAME_PROCESSOR_MODULE_DIR/build/outputs/aar/vesper-player-kit-frame-processor-diagnostic-release.aar"
    FRAME_PROCESSOR_OUTPUT_AAR="$OUTPUT_DIR/VesperPlayerKitFrameProcessorDiagnostic-android-$abi.aar"
    cp "$FRAME_PROCESSOR_INPUT_AAR" "$FRAME_PROCESSOR_OUTPUT_AAR"
    frame_processor_entries="$(unzip -Z1 "$FRAME_PROCESSOR_OUTPUT_AAR")"
    if grep -E '(^|/)jni/[^/]+/(libav|libsw|libxml2|libssl|libcrypto).*\.so$' <<<"$frame_processor_entries" >/dev/null; then
      echo "Android FrameProcessor diagnostic AAR must not bundle FFmpeg runtime libraries:" >&2
      grep -E '(^|/)jni/[^/]+/(libav|libsw|libxml2|libssl|libcrypto).*\.so$' <<<"$frame_processor_entries" >&2
      exit 1
    fi
  fi

  release_aars=(
    "$CORE_OUTPUT_AAR"
    "$COMPOSE_OUTPUT_AAR"
    "$COMPOSE_UI_OUTPUT_AAR"
  )
  if [[ "$include_optional_plugins" -eq 1 ]]; then
    release_aars+=(
      "$EXTERNAL_PLAYBACK_OUTPUT_AAR"
      "$FFMPEG_RUNTIME_OUTPUT_AAR"
      "$DECODER_MEDIACODEC_OUTPUT_AAR"
      "$SOURCE_NORMALIZER_OUTPUT_AAR"
      "$FRAME_PROCESSOR_OUTPUT_AAR"
    )
  fi
  for release_aar in "${release_aars[@]}"; do
    vesper_verify_archive_excludes_test_fixtures "$release_aar"
  done

  echo "Staged VesperPlayerKit Android AARs:"
  echo "  $CORE_OUTPUT_AAR"
  echo "  $COMPOSE_OUTPUT_AAR"
  echo "  $COMPOSE_UI_OUTPUT_AAR"
  if [[ "$include_optional_plugins" -eq 1 ]]; then
    echo "  $EXTERNAL_PLAYBACK_OUTPUT_AAR"
    echo "  $FFMPEG_RUNTIME_OUTPUT_AAR"
    echo "  $DECODER_MEDIACODEC_OUTPUT_AAR"
    echo "  $SOURCE_NORMALIZER_OUTPUT_AAR"
    echo "  $FRAME_PROCESSOR_OUTPUT_AAR"
  else
    echo "Skipped optional Android plugin AARs. Set VESPER_ANDROID_INCLUDE_OPTIONAL_PLUGINS=1 to stage them."
  fi
done
