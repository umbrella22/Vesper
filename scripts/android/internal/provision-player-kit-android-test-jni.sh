#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../lib" && pwd)/android.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../lib" && pwd)/ffmpeg.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../lib" && pwd)/ffmpeg-profile.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="${1:-}"
BUILD_PROFILE="${2:-debug}"
FFMPEG_PROFILE="${3:-default}"
EXPECTED_OUTPUT_DIR="$ROOT_DIR/lib/android/vesper-player-kit/build/generated/androidTestJniLibs"

usage() {
  cat <<EOF >&2
Usage: $0 <output-dir> [debug|release] [ffmpeg-profile]

Android ABI selection is controlled by RUST_ANDROID_ABIS.
EOF
}

if [[ -z "$OUTPUT_DIR" ]]; then
  usage
  exit 1
fi
if [[ "$OUTPUT_DIR" != "$EXPECTED_OUTPUT_DIR" ]]; then
  echo "Android instrumentation JNI output must stay inside the player-kit generated build directory:" >&2
  echo "  $EXPECTED_OUTPUT_DIR" >&2
  exit 1
fi
case "$BUILD_PROFILE" in
  debug|release)
    ;;
  *)
    usage
    exit 1
    ;;
esac

selected_abis=()
resolved_abis="$(vesper_android_resolve_selected_abis)"
while IFS= read -r abi; do
  selected_abis+=("$abi")
done <<<"$resolved_abis"
selected_abis_csv="$(IFS=,; echo "${selected_abis[*]}")"

temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/vesper-player-kit-android-test-jni.XXXXXX")"
trap 'rm -rf "$temp_dir"' EXIT
decoder_dir="$temp_dir/decoder"
source_normalizer_dir="$temp_dir/source-normalizer"
source_normalizer_metadata_dir="$temp_dir/source-normalizer-metadata"
runtime_dir="$temp_dir/runtime"
combined_dir="$temp_dir/combined"

RUST_ANDROID_ABIS="$selected_abis_csv" \
  "$ROOT_DIR/scripts/android/build-player-decoder-mediacodec-plugin.sh" \
    "$decoder_dir" \
    "$BUILD_PROFILE"
RUST_ANDROID_ABIS="$selected_abis_csv" \
  "$ROOT_DIR/scripts/android/build-player-source-normalizer-ffmpeg-plugin.sh" \
    "$source_normalizer_dir" \
    "$BUILD_PROFILE" \
    --profile "$FFMPEG_PROFILE" \
    --metadata-dir "$source_normalizer_metadata_dir"

FFMPEG_ARGS=()
vesper_ffmpeg_profile_resolve "$FFMPEG_PROFILE" android
vesper_ffmpeg_validate_resolved_profile \
  "$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]+"${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]}"})" \
  "$VESPER_PROFILE_RESOLVED_TLS_BACKEND" \
  "${VESPER_PROFILE_VALIDATION_FORBID_NETWORK:-false}" \
  "${VESPER_PROFILE_VALIDATION_FORBID_OPENSSL:-false}" \
  ${VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS[@]+"${VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS[@]}"}
while IFS= read -r arg; do
  FFMPEG_ARGS+=("$arg")
done < <(vesper_ffmpeg_profile_emit_legacy_args)
vesper_ffmpeg_profile_export_validation_env
vesper_ffmpeg_parse_common_args android "${FFMPEG_ARGS[@]}"

FFMPEG_ANDROID_DIR="${VESPER_ANDROID_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir android "$ROOT_DIR/third_party/ffmpeg/android")}}"
OPENSSL_ANDROID_DIR="${VESPER_ANDROID_OPENSSL_OUTPUT_DIR:-$ROOT_DIR/third_party/openssl/android}"
LIBXML2_ANDROID_DIR="${VESPER_ANDROID_LIBXML2_OUTPUT_DIR:-$ROOT_DIR/third_party/libxml2/android}"
vesper_android_stage_ffmpeg_runtime_libraries \
  "$runtime_dir" \
  "$FFMPEG_ANDROID_DIR" \
  "$OPENSSL_ANDROID_DIR" \
  "$LIBXML2_ANDROID_DIR" \
  "$VESPER_FFMPEG_USE_OPENSSL" \
  "$VESPER_FFMPEG_USE_LIBXML2" \
  "${selected_abis[@]}"

mkdir -p "$combined_dir"
cp -R "$decoder_dir"/. "$combined_dir"/
cp -R "$source_normalizer_dir"/. "$combined_dir"/
cp -R "$runtime_dir"/. "$combined_dir"/

for abi in "${selected_abis[@]}"; do
  required_libraries=(
    libvesper_decoder_mediacodec.so
    libvesper_source_normalizer_ffmpeg.so
    libavcodec.so
    libavformat.so
    libavutil.so
  )
  if [[ "$VESPER_FFMPEG_USE_LIBXML2" == "1" ]]; then
    required_libraries+=(libxml2.so)
  fi
  if [[ "$VESPER_FFMPEG_USE_OPENSSL" == "1" ]]; then
    required_libraries+=(libssl.so libcrypto.so)
  fi
  for library in "${required_libraries[@]}"; do
    if [[ ! -f "$combined_dir/$abi/$library" ]]; then
      echo "Android instrumentation JNI provisioning is missing $library for ABI $abi." >&2
      exit 1
    fi
  done
done

mkdir -p "$(dirname "$OUTPUT_DIR")"
rm -rf "$OUTPUT_DIR"
mv "$combined_dir" "$OUTPUT_DIR"

echo
echo "Provisioned Android instrumentation JNI libraries into:"
echo "  $OUTPUT_DIR"
