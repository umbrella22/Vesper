#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
ANDROID_NDK_VERSION="29.0.14206865"
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-}"
ANDROID_API_LEVEL="${VESPER_ANDROID_FFMPEG_ANDROID_API:-26}"
FFMPEG_VERSION="${VESPER_ANDROID_FFMPEG_VERSION:-8.1}"
FFMPEG_ARCHIVE_NAME="ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_SOURCE_URL="${VESPER_ANDROID_FFMPEG_SOURCE_URL:-https://ffmpeg.org/releases/${FFMPEG_ARCHIVE_NAME}}"
FFMPEG_SOURCE_ARCHIVE="${VESPER_ANDROID_FFMPEG_SOURCE_ARCHIVE:-$ROOT_DIR/${FFMPEG_ARCHIVE_NAME}}"
FFMPEG_OUTPUT_DIR="${VESPER_ANDROID_FFMPEG_OUTPUT_DIR:-$ROOT_DIR/third_party/ffmpeg/android}"
TLS_BACKEND="${VESPER_ANDROID_FFMPEG_TLS_BACKEND:-openssl}"
ENABLE_DASH="${VESPER_ANDROID_FFMPEG_ENABLE_DASH:-1}"
OPENSSL_ANDROID_DIR="${ROOT_DIR}/third_party/openssl/android"
LIBXML2_ANDROID_DIR="${ROOT_DIR}/third_party/libxml2/android"
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

map_abi_to_ffmpeg_arch() {
  case "$1" in
    arm64-v8a)
      echo "aarch64"
      ;;
    x86_64)
      echo "x86_64"
      ;;
    *)
      return 1
      ;;
  esac
}

map_abi_to_ffmpeg_cpu() {
  case "$1" in
    arm64-v8a)
      echo "armv8-a"
      ;;
    x86_64)
      echo "x86_64"
      ;;
    *)
      return 1
      ;;
  esac
}

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

resolve_host_tag() {
  local os
  local arch

  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin)
      if [[ "$arch" == "arm64" ]]; then
        if [[ -d "$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/darwin-arm64" ]]; then
          echo "darwin-arm64"
          return 0
        fi
      fi
      echo "darwin-x86_64"
      ;;
    Linux)
      echo "linux-x86_64"
      ;;
    *)
      echo "Unsupported host OS: $os" >&2
      return 1
      ;;
  esac
}

download_if_missing() {
  local archive_path="$1"
  local archive_url="$2"

  if [[ -f "$archive_path" ]]; then
    return 0
  fi

  if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required to download FFmpeg source archives." >&2
    exit 1
  fi

  mkdir -p "$(dirname "$archive_path")"
  echo "Downloading FFmpeg source archive:"
  echo "  $archive_url"
  curl --fail --location --silent --show-error --output "$archive_path" "$archive_url"
}

ensure_dependency_dir() {
  local path="$1"
  local message="$2"

  if [[ ! -d "$path" ]]; then
    echo "$message" >&2
    exit 1
  fi
}

resolve_make_jobs() {
  if command -v getconf >/dev/null 2>&1; then
    getconf _NPROCESSORS_ONLN
    return 0
  fi

  if command -v sysctl >/dev/null 2>&1; then
    sysctl -n hw.ncpu
    return 0
  fi

  echo 4
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

if ! ANDROID_NDK_ROOT="$(resolve_ndk_root)"; then
  echo "Android NDK is missing or incomplete at:" >&2
  echo "  $ANDROID_SDK_ROOT/ndk/$ANDROID_NDK_VERSION" >&2
  echo >&2
  echo "Install Android NDK $ANDROID_NDK_VERSION from Android Studio." >&2
  exit 1
fi

HOST_TAG="$(resolve_host_tag)"
TOOLCHAIN_ROOT="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/$HOST_TAG"
TOOLCHAIN_BIN_DIR="$TOOLCHAIN_ROOT/bin"
SYSROOT="$TOOLCHAIN_ROOT/sysroot"
MAKE_JOBS="$(resolve_make_jobs)"

if [[ ! -d "$TOOLCHAIN_BIN_DIR" ]]; then
  echo "Android LLVM toolchain is missing at:" >&2
  echo "  $TOOLCHAIN_BIN_DIR" >&2
  exit 1
fi

case "$TLS_BACKEND" in
  openssl)
    ;;
  *)
    echo "Unsupported Android FFmpeg TLS backend: $TLS_BACKEND" >&2
    echo "Supported values: openssl" >&2
    exit 1
    ;;
esac

download_if_missing "$FFMPEG_SOURCE_ARCHIVE" "$FFMPEG_SOURCE_URL"

temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT

tar -xf "$FFMPEG_SOURCE_ARCHIVE" -C "$temp_dir"
FFMPEG_SOURCE_DIR="$(find "$temp_dir" -mindepth 1 -maxdepth 1 -type d | head -n 1)"

if [[ -z "$FFMPEG_SOURCE_DIR" || ! -f "$FFMPEG_SOURCE_DIR/configure" ]]; then
  echo "Unable to locate FFmpeg source tree extracted from:" >&2
  echo "  $FFMPEG_SOURCE_ARCHIVE" >&2
  exit 1
fi

mkdir -p "$FFMPEG_OUTPUT_DIR"

for abi in "${selected_abis[@]}"; do
  ffmpeg_arch="$(map_abi_to_ffmpeg_arch "$abi")"
  ffmpeg_cpu="$(map_abi_to_ffmpeg_cpu "$abi")"
  toolchain_target="$(map_abi_to_rust_target "$abi")"
  cc="$TOOLCHAIN_BIN_DIR/${toolchain_target}${ANDROID_API_LEVEL}-clang"
  cxx="$TOOLCHAIN_BIN_DIR/${toolchain_target}${ANDROID_API_LEVEL}-clang++"
  install_dir="$FFMPEG_OUTPUT_DIR/$abi"
  build_dir="$temp_dir/build-$abi"
  openssl_dir="$OPENSSL_ANDROID_DIR/$abi"
  libxml2_dir="$LIBXML2_ANDROID_DIR/$abi"
  pkg_config_paths=()
  configure_args=()
  extra_cflags=(-fPIC)
  extra_ldflags=(-Wl,-z,max-page-size=16384)

  ensure_dependency_dir "$openssl_dir/lib/pkgconfig" "Missing Android OpenSSL prebuilt for ABI $abi: $openssl_dir"
  pkg_config_paths+=("$openssl_dir/lib/pkgconfig")
  extra_cflags+=("-I$openssl_dir/include")
  extra_ldflags+=("-L$openssl_dir/lib")
  configure_args+=(--enable-openssl --enable-version3)

  if [[ "$ENABLE_DASH" == "1" ]]; then
    ensure_dependency_dir "$libxml2_dir/lib/pkgconfig" "Missing Android libxml2 prebuilt for ABI $abi: $libxml2_dir"
    pkg_config_paths+=("$libxml2_dir/lib/pkgconfig")
    extra_cflags+=("-I$libxml2_dir/include")
    extra_ldflags+=("-L$libxml2_dir/lib")
    configure_args+=(--enable-libxml2)
  fi

  rm -rf "$install_dir" "$build_dir"
  mkdir -p "$install_dir" "$build_dir"

  echo "Building Android FFmpeg prebuilt for $abi"

  (
    export PKG_CONFIG_ALLOW_CROSS=1
    export PKG_CONFIG_PATH
    PKG_CONFIG_PATH="$(IFS=:; echo "${pkg_config_paths[*]}")"

    extra_cflags_value="$(IFS=' '; echo "${extra_cflags[*]}")"
    extra_ldflags_value="$(IFS=' '; echo "${extra_ldflags[*]}")"

    cd "$build_dir"
    "$FFMPEG_SOURCE_DIR/configure" \
      --prefix="$install_dir" \
      --target-os=android \
      --arch="$ffmpeg_arch" \
      --cpu="$ffmpeg_cpu" \
      --sysroot="$SYSROOT" \
      --cc="$cc" \
      --cxx="$cxx" \
      --ld="$cc" \
      --ar="$TOOLCHAIN_BIN_DIR/llvm-ar" \
      --nm="$TOOLCHAIN_BIN_DIR/llvm-nm" \
      --ranlib="$TOOLCHAIN_BIN_DIR/llvm-ranlib" \
      --strip="$TOOLCHAIN_BIN_DIR/llvm-strip" \
      --as="$cc" \
      --enable-cross-compile \
      --disable-programs \
      --disable-doc \
      --disable-debug \
      --disable-static \
      --enable-shared \
      --disable-x86asm \
      --enable-network \
      --extra-cflags="$extra_cflags_value" \
      --extra-ldflags="$extra_ldflags_value" \
      "${configure_args[@]}"

    make -j"$MAKE_JOBS"
    make install
  )

  rm -rf "$install_dir/bin" "$install_dir/share"
done

echo
echo "Built Android FFmpeg prebuilts into:"
echo "  $FFMPEG_OUTPUT_DIR"
echo "Using FFmpeg source archive:"
echo "  $FFMPEG_SOURCE_ARCHIVE"
echo "Selected Android ABIs:"
for abi in "${selected_abis[@]}"; do
  echo "  $abi"
done
