#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
PROJECT_DIR="$ROOT_DIR/lib/android"
RUNTIME_MODULE_DIR="$PROJECT_DIR/vesper-player-kit-ffmpeg-runtime"
JNI_LIBS_DIR="$RUNTIME_MODULE_DIR/src/main/jniLibs"
ASSETS_DIR="$RUNTIME_MODULE_DIR/src/main/assets/vesper-ffmpeg-runtime"
PROJECT_GRADLEW="$PROJECT_DIR/gradlew"
LOCAL_GRADLE="$(find "$PROJECT_DIR/.gradle/wrapper/dists" -path '*/bin/gradle' -type f -perm -111 2>/dev/null | sort | tail -n 1 || true)"
FALLBACK_GRADLEW="$ROOT_DIR/examples/android-compose-host/gradlew"

FFMPEG_ARGS=()
while IFS= read -r arg; do
  FFMPEG_ARGS+=("$arg")
done < <("$ROOT_DIR/scripts/android/resolve-ffmpeg-runtime-requirements.sh" "$@")
BUILD_CONSUMERS=("$@")
if [[ ${#BUILD_CONSUMERS[@]} -eq 0 ]]; then
  BUILD_CONSUMERS=(download-remux relay-remux)
fi

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
if [[ -x "$PROJECT_GRADLEW" ]]; then
  GRADLE_CMD=("$PROJECT_GRADLEW")
elif [[ -n "$LOCAL_GRADLE" && -x "$LOCAL_GRADLE" ]]; then
  GRADLE_CMD=("$LOCAL_GRADLE")
elif [[ -x "$FALLBACK_GRADLEW" ]]; then
  GRADLE_CMD=("$FALLBACK_GRADLEW")
else
  echo "No Gradle wrapper or local Gradle distribution was found for Android runtime AAR." >&2
  exit 1
fi

"${GRADLE_CMD[@]}" -p "$PROJECT_DIR" :vesper-player-kit-ffmpeg-runtime:assembleRelease

for consumer in "${BUILD_CONSUMERS[@]}"; do
  if [[ "$consumer" == "relay-remux" ]]; then
    "$ROOT_DIR/scripts/android/verify-relay-ffmpeg-runtime-no-network.sh" "${BUILD_CONSUMERS[@]}"
    break
  fi
done

echo
echo "Built Android FFmpeg runtime AAR with consumers: ${*:-download-remux relay-remux}"
echo "Runtime JNI libs:"
echo "  $JNI_LIBS_DIR"
