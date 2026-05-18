#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-profile.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="${1:-}"
FFMPEG_PROFILE="download-remux"

if [[ -z "$OUTPUT_DIR" ]]; then
  echo "Usage: $0 <output-dir> [debug|release] [--profile <name>]" >&2
  echo "Default FFmpeg profile: download-remux." >&2
  echo "Android ABI selection is controlled by RUST_ANDROID_ABIS." >&2
  exit 1
fi

shift || true

BUILD_PROFILE="debug"
if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
  BUILD_PROFILE="$1"
  shift
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      [[ -n "${2:-}" ]] || { echo "--profile requires a value." >&2; exit 1; }
      FFMPEG_PROFILE="$2"
      shift 2
      ;;
    --profile=*)
      FFMPEG_PROFILE="${1#*=}"
      shift
      ;;
    *)
      echo "Unexpected arguments: $*" >&2
      echo "Use --profile to select a declared FFmpeg profile." >&2
      exit 1
      ;;
  esac
done

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
PROFILE_HASH="$(vesper_ffmpeg_profile_key android)"

ffmpeg_build_args=(
  ffmpeg
  --platform android
  --profile "$FFMPEG_PROFILE"
  --android-artifact prebuilts
)
if [[ -n "${RUST_ANDROID_ABIS:-}" ]]; then
  ffmpeg_build_args+=(--abi "$RUST_ANDROID_ABIS")
fi
"$ROOT_DIR/scripts/vesper" "${ffmpeg_build_args[@]}"

ANDROID_SDK_ROOT="$(vesper_android_sdk_root)"
ANDROID_NDK_VERSION="$(vesper_android_ndk_version)"
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-}"

selected_abis=()
while IFS= read -r abi; do
  selected_abis+=("$abi")
done < <(vesper_android_resolve_selected_abis)

required_targets=()
for abi in "${selected_abis[@]}"; do
  required_targets+=("$(vesper_android_abi_to_rust_target "$abi")")
done

vesper_android_require_cargo_ndk "Android player-remux-ffmpeg plugins"
vesper_android_require_rust_targets ${required_targets[@]+"${required_targets[@]}"}

if ! ANDROID_NDK_ROOT="$(vesper_android_resolve_ndk_root "$ANDROID_SDK_ROOT" "$ANDROID_NDK_ROOT" "$ANDROID_NDK_VERSION")"; then
  vesper_android_report_missing_ndk "$ANDROID_SDK_ROOT" "$ANDROID_NDK_VERSION"
  exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

for abi in "${selected_abis[@]}"; do
  ffmpeg_abi_dir="$FFMPEG_ANDROID_DIR/$abi"
  pkgconfig_dir="$ffmpeg_abi_dir/lib/pkgconfig"
  metadata_path="$ffmpeg_abi_dir/vesper-ffmpeg-build-metadata.txt"

  if [[ ! -d "$pkgconfig_dir" ]]; then
    echo "Missing shared FFmpeg runtime pkg-config directory for ABI $abi:" >&2
    echo "  $pkgconfig_dir" >&2
    exit 1
  fi

  configure_metadata=""
  if [[ -f "$metadata_path" ]]; then
    configure_metadata="$(tr '\n' ';' <"$metadata_path")"
  fi

  cargo_args=(
    ndk
    -o "$OUTPUT_DIR"
    -t "$abi"
    build
    -p player-remux-ffmpeg
  )
  if [[ "$BUILD_PROFILE" == "release" ]]; then
    cargo_args+=(--release)
  fi

  env \
    PKG_CONFIG_ALLOW_CROSS=1 \
    PKG_CONFIG_PATH="$pkgconfig_dir" \
    VESPER_FFMPEG_PROFILE_HASH="$PROFILE_HASH" \
    VESPER_FFMPEG_CONFIGURE_METADATA="$configure_metadata" \
    cargo "${cargo_args[@]}"
done

unexpected_runtime="$(
  find "$OUTPUT_DIR" -type f \
    \( -name 'libav*.so' -o -name 'libsw*.so' -o -name 'libssl*.so' -o -name 'libcrypto*.so' -o -name 'libxml2*.so' \) \
    -print -quit
)"
if [[ -n "$unexpected_runtime" ]]; then
  echo "player-remux-ffmpeg must not bundle FFmpeg runtime libraries:" >&2
  echo "  $unexpected_runtime" >&2
  echo "Package vesper-player-kit-ffmpeg-runtime with a union profile instead." >&2
  exit 1
fi

echo
echo "Built Android player-remux-ffmpeg plugin libraries into:"
echo "  $OUTPUT_DIR"
echo "FFmpeg profile:"
echo "  $FFMPEG_PROFILE"
echo "The plugin no longer copies FFmpeg runtime libraries; package vesper-player-kit-ffmpeg-runtime instead."
