#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FFMPEG_ANDROID_DIR="$ROOT_DIR/third_party/ffmpeg/android"
OPENSSL_ANDROID_DIR="$ROOT_DIR/third_party/openssl/android"
LIBXML2_ANDROID_DIR="$ROOT_DIR/third_party/libxml2/android"
OUTPUT_DIR="${1:-}"

if [[ -z "$OUTPUT_DIR" ]]; then
  echo "Usage: $0 <output-dir> [debug|release] [abi...]" >&2
  exit 1
fi

shift || true

PROFILE="debug"
if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
  PROFILE="$1"
  shift
fi

ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
ANDROID_NDK_VERSION="29.0.14206865"
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-}"
DEFAULT_ABIS=(
  "arm64-v8a"
  "x86_64"
)

resolve_selected_abis() {
  local -a resolved=()
  local token

  if [[ $# -gt 0 ]]; then
    resolved=("$@")
  elif [[ -n "${RUST_ANDROID_ABIS:-}" ]]; then
    read -r -a resolved <<<"${RUST_ANDROID_ABIS//,/ }"
  else
    resolved=("${DEFAULT_ABIS[@]}")
  fi

  if [[ ${#resolved[@]} -eq 0 ]]; then
    echo "No Android ABIs were selected." >&2
    exit 1
  fi

  for token in "${resolved[@]}"; do
    case "$token" in
      arm64-v8a|x86_64)
        ;;
      *)
        echo "Unsupported Android ABI: $token" >&2
        echo "Supported ABIs: arm64-v8a, x86_64" >&2
        exit 1
        ;;
    esac
  done

  printf '%s\n' "${resolved[@]}"
}

map_abi_to_rust_target() {
  case "$1" in
    arm64-v8a)
      echo "aarch64-linux-android"
      ;;
    x86_64)
      echo "x86_64-linux-android"
      ;;
    *)
      return 1
      ;;
  esac
}

selected_abis=()
while IFS= read -r abi; do
  selected_abis+=("$abi")
done < <(resolve_selected_abis "$@")

required_targets=()
for abi in "${selected_abis[@]}"; do
  required_targets+=("$(map_abi_to_rust_target "$abi")")
done

installed_targets="$(rustup target list --installed)"

if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "cargo-ndk is required to build Android player-ffmpeg plugins." >&2
  echo "Install it with: cargo install cargo-ndk" >&2
  exit 1
fi

missing_targets=()
for target in "${required_targets[@]}"; do
  if [[ "$installed_targets" != *"$target"* ]]; then
    missing_targets+=("$target")
  fi
done

if [[ ${#missing_targets[@]} -gt 0 ]]; then
  echo "Required Rust Android targets are missing:" >&2
  for target in "${missing_targets[@]}"; do
    echo "  $target" >&2
  done
  echo >&2
  echo "Install them with:" >&2
  echo "  rustup target add ${missing_targets[*]}" >&2
  exit 1
fi

resolve_ndk_root() {
  local candidate

  if [[ -n "$ANDROID_NDK_ROOT" ]]; then
    echo "$ANDROID_NDK_ROOT"
    return 0
  fi

  candidate="$ANDROID_SDK_ROOT/ndk/$ANDROID_NDK_VERSION"
  if [[ -f "$candidate/source.properties" ]]; then
    echo "$candidate"
    return 0
  fi

  if [[ -d "$ANDROID_SDK_ROOT/ndk" ]]; then
    while IFS= read -r candidate; do
      if [[ -f "$candidate/source.properties" ]]; then
        echo "$candidate"
        return 0
      fi
    done < <(find "$ANDROID_SDK_ROOT/ndk" -mindepth 1 -maxdepth 1 -type d | sort -Vr)
  fi

  return 1
}

if ! ANDROID_NDK_ROOT="$(resolve_ndk_root)"; then
  echo "Android NDK is missing or incomplete at:" >&2
  echo "  $ANDROID_SDK_ROOT/ndk/$ANDROID_NDK_VERSION" >&2
  echo >&2
  echo "Expected a complete NDK installation containing:" >&2
  echo "  <ndk-dir>/source.properties" >&2
  echo >&2
  echo "Install Android NDK $ANDROID_NDK_VERSION from Android Studio." >&2
  exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

missing_ffmpeg_abis=()
for abi in "${selected_abis[@]}"; do
  if [[ ! -f "$FFMPEG_ANDROID_DIR/$abi/lib/pkgconfig/libavformat.pc" ]]; then
    missing_ffmpeg_abis+=("$abi")
  fi
done

if [[ ${#missing_ffmpeg_abis[@]} -gt 0 ]]; then
  "$ROOT_DIR/scripts/build-android-ffmpeg-prebuilts.sh" "${missing_ffmpeg_abis[@]}"
fi

for abi in "${selected_abis[@]}"; do
  ffmpeg_abi_dir="$FFMPEG_ANDROID_DIR/$abi"
  pkgconfig_dir="$ffmpeg_abi_dir/lib/pkgconfig"

  if [[ ! -d "$pkgconfig_dir" ]]; then
    echo "Missing FFmpeg pkg-config directory for ABI $abi:" >&2
    echo "  $pkgconfig_dir" >&2
    exit 1
  fi

  if [[ "$PROFILE" == "release" ]]; then
    env \
      PKG_CONFIG_ALLOW_CROSS=1 \
      PKG_CONFIG_PATH="$pkgconfig_dir" \
      cargo ndk \
        -o "$OUTPUT_DIR" \
        -t "$abi" \
        build \
        -p player-ffmpeg \
        --release
  else
    env \
      PKG_CONFIG_ALLOW_CROSS=1 \
      PKG_CONFIG_PATH="$pkgconfig_dir" \
      cargo ndk \
        -o "$OUTPUT_DIR" \
        -t "$abi" \
        build \
        -p player-ffmpeg
  fi

  mkdir -p "$OUTPUT_DIR/$abi"
  find "$ffmpeg_abi_dir/lib" -maxdepth 1 -type f -name 'lib*.so' -exec cp {} "$OUTPUT_DIR/$abi/" \;

  for runtime_dependency in \
    "$OPENSSL_ANDROID_DIR/$abi/lib/libssl.so" \
    "$OPENSSL_ANDROID_DIR/$abi/lib/libcrypto.so" \
    "$LIBXML2_ANDROID_DIR/$abi/lib/libxml2.so"; do
    if [[ -f "$runtime_dependency" ]]; then
      cp "$runtime_dependency" "$OUTPUT_DIR/$abi/"
    fi
  done
done

echo
echo "Built Android player-ffmpeg plugin libraries into:"
echo "  $OUTPUT_DIR"
echo "Selected Android ABIs:"
for abi in "${selected_abis[@]}"; do
  echo "  $abi"
done
