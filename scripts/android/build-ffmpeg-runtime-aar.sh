#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/android"
RUNTIME_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-ffmpeg-runtime"
JNI_LIBS_DIR="$RUNTIME_MODULE_DIR/src/main/jniLibs"
ASSETS_DIR="$RUNTIME_MODULE_DIR/src/main/assets/vesper-ffmpeg-runtime"
FALLBACK_PROJECT_DIR="$ROOT_DIR/examples/android-compose-host"

if [[ $# -eq 0 || "${1:0:2}" != "--" ]]; then
  cat <<EOF >&2
Android FFmpeg runtime AAR builds require resolved FFmpeg arguments.

Use the root profile CLI instead:
  ./scripts/vesper ffmpeg --platform android --profile default
EOF
  exit 1
fi

FFMPEG_ARGS=("$@")

vesper_ffmpeg_parse_common_args android "${FFMPEG_ARGS[@]}"
FFMPEG_OUTPUT_DIR="${VESPER_ANDROID_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir android "$ROOT_DIR/third_party/ffmpeg/android")}}"
PROFILE_HASH="$(vesper_ffmpeg_profile_key android)"
OPENSSL_ANDROID_DIR="${VESPER_ANDROID_OPENSSL_OUTPUT_DIR:-$ROOT_DIR/third_party/openssl/android}"
LIBXML2_ANDROID_DIR="${VESPER_ANDROID_LIBXML2_OUTPUT_DIR:-$ROOT_DIR/third_party/libxml2/android}"

"$ROOT_DIR/scripts/android/build-ffmpeg-prebuilts.sh" "${FFMPEG_ARGS[@]}"

selected_abis=()
while IFS= read -r abi; do
  selected_abis+=("$abi")
done < <(vesper_android_resolve_selected_abis ${VESPER_FFMPEG_POSITIONAL_ARGS[@]+"${VESPER_FFMPEG_POSITIONAL_ARGS[@]}"})

rm -rf "$JNI_LIBS_DIR" "$ASSETS_DIR"
mkdir -p "$JNI_LIBS_DIR" "$ASSETS_DIR"
for abi in "${selected_abis[@]}"; do
  mkdir -p "$JNI_LIBS_DIR/$abi"
  find "$FFMPEG_OUTPUT_DIR/$abi/lib" -maxdepth 1 -type f -name 'lib*.so' -exec cp {} "$JNI_LIBS_DIR/$abi/" \;
  if [[ "$VESPER_FFMPEG_USE_OPENSSL" == "1" && -d "$OPENSSL_ANDROID_DIR/$abi/lib" ]]; then
    find "$OPENSSL_ANDROID_DIR/$abi/lib" -maxdepth 1 -type f \( -name 'libssl*.so' -o -name 'libcrypto*.so' \) -exec cp {} "$JNI_LIBS_DIR/$abi/" \;
  fi
  if [[ "$VESPER_FFMPEG_USE_LIBXML2" == "1" && -d "$LIBXML2_ANDROID_DIR/$abi/lib" ]]; then
    find "$LIBXML2_ANDROID_DIR/$abi/lib" -maxdepth 1 -type f -name 'libxml2*.so' -exec cp {} "$JNI_LIBS_DIR/$abi/" \;
  fi
  if [[ -f "$FFMPEG_OUTPUT_DIR/$abi/vesper-ffmpeg-build-metadata.txt" ]]; then
    cp "$FFMPEG_OUTPUT_DIR/$abi/vesper-ffmpeg-build-metadata.txt" "$ASSETS_DIR/$abi-metadata.txt"
    printf 'profile_hash=%s\n' "$PROFILE_HASH" >>"$ASSETS_DIR/$abi-metadata.txt"
  fi
done
printf '%s\n' "$PROFILE_HASH" >"$ASSETS_DIR/profile-hash.txt"

export GRADLE_USER_HOME="${GRADLE_USER_HOME:-$ROOT_DIR/.gradle/gradle-user-home}"
GRADLE_CMD=("$(vesper_android_resolve_gradle "$PROJECT_DIR" "$FALLBACK_PROJECT_DIR")")

"${GRADLE_CMD[@]}" -p "$PROJECT_DIR" :vesper-player-kit-ffmpeg-runtime:assembleRelease

if [[ "${VESPER_FFMPEG_VALIDATION_FORBID_NETWORK:-false}" == "true" || "${VESPER_FFMPEG_VALIDATION_FORBID_OPENSSL:-false}" == "true" ]]; then
  vesper_ffmpeg_validate_android_runtime_artifacts \
    "$RUNTIME_MODULE_DIR" \
    "${VESPER_FFMPEG_VALIDATION_FORBID_NETWORK:-false}" \
    "${VESPER_FFMPEG_VALIDATION_FORBID_OPENSSL:-false}"
fi

echo
echo "Built Android FFmpeg runtime AAR for resolved profile: ${VESPER_DECLARED_FFMPEG_PROFILE:-$VESPER_FFMPEG_PROFILE}"
echo "Runtime JNI libs:"
echo "  $JNI_LIBS_DIR"
