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
OPENSSL_VERSION="${VESPER_ANDROID_OPENSSL_VERSION:-3.6.1}"
OPENSSL_SERIES="${OPENSSL_VERSION%.*}"
OPENSSL_ARCHIVE_NAME="openssl-${OPENSSL_VERSION}.tar.gz"
OPENSSL_SOURCE_URL="${VESPER_ANDROID_OPENSSL_SOURCE_URL:-https://www.openssl-library.org/source/${OPENSSL_ARCHIVE_NAME}}"
OPENSSL_SOURCE_ARCHIVE="${VESPER_ANDROID_OPENSSL_SOURCE_ARCHIVE:-$ROOT_DIR/third_party/openssl/android/prebuilt-archives/${OPENSSL_ARCHIVE_NAME}}"
LIBXML2_VERSION="${VESPER_ANDROID_LIBXML2_VERSION:-2.14.6}"
LIBXML2_SERIES="${LIBXML2_VERSION%.*}"
LIBXML2_ARCHIVE_NAME="libxml2-${LIBXML2_VERSION}.tar.xz"
LIBXML2_SOURCE_URL="${VESPER_ANDROID_LIBXML2_SOURCE_URL:-https://download.gnome.org/sources/libxml2/${LIBXML2_SERIES}/${LIBXML2_ARCHIVE_NAME}}"
LIBXML2_SOURCE_ARCHIVE="${VESPER_ANDROID_LIBXML2_SOURCE_ARCHIVE:-$ROOT_DIR/third_party/libxml2/android/prebuilt-archives/${LIBXML2_ARCHIVE_NAME}}"
TLS_BACKEND="${VESPER_ANDROID_FFMPEG_TLS_BACKEND:-openssl}"
ENABLE_DASH="${VESPER_ANDROID_FFMPEG_ENABLE_DASH:-1}"
OPENSSL_ANDROID_DIR="${VESPER_ANDROID_OPENSSL_OUTPUT_DIR:-$ROOT_DIR/third_party/openssl/android}"
LIBXML2_ANDROID_DIR="${VESPER_ANDROID_LIBXML2_OUTPUT_DIR:-$ROOT_DIR/third_party/libxml2/android}"
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

map_abi_to_openssl_target() {
  case "$1" in
    arm64-v8a)
      echo "android-arm64"
      ;;
    x86_64)
      echo "android-x86_64"
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
  shift
  local archive_url
  local download_succeeded=0

  if [[ -f "$archive_path" ]]; then
    return 0
  fi

  if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required to download source archives." >&2
    exit 1
  fi

  mkdir -p "$(dirname "$archive_path")"

  for archive_url in "$@"; do
    echo "Downloading source archive:"
    echo "  $archive_url"
    if curl --fail --location --silent --show-error --output "$archive_path" "$archive_url"; then
      download_succeeded=1
      break
    fi

    rm -f "$archive_path"
    echo "Source download failed, trying next mirror if available." >&2
  done

  if [[ "$download_succeeded" != "1" ]]; then
    echo "Unable to download source archive into:" >&2
    echo "  $archive_path" >&2
    echo "Tried source URLs:" >&2
    for archive_url in "$@"; do
      echo "  $archive_url" >&2
    done
    exit 1
  fi
}

ensure_command() {
  local command_name="$1"

  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing required command: $command_name" >&2
    exit 1
  fi
}

ensure_dependency_dir() {
  local path="$1"
  local message="$2"

  if [[ ! -d "$path" ]]; then
    echo "$message" >&2
    exit 1
  fi
}

extract_source_tree() {
  local archive_path="$1"
  local destination_dir="$2"

  rm -rf "$destination_dir"
  mkdir -p "$destination_dir"
  tar -xf "$archive_path" -C "$destination_dir" --strip-components=1
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

build_android_openssl_prebuilt() {
  local abi="$1"
  local openssl_target="$2"
  local toolchain_target="$3"
  local install_dir="$OPENSSL_ANDROID_DIR/$abi"
  local source_dir="$temp_dir/openssl-$abi"
  local cc="$TOOLCHAIN_BIN_DIR/${toolchain_target}${ANDROID_API_LEVEL}-clang"
  local cxx="$TOOLCHAIN_BIN_DIR/${toolchain_target}${ANDROID_API_LEVEL}-clang++"

  ensure_command perl
  ensure_command make
  download_if_missing \
    "$OPENSSL_SOURCE_ARCHIVE" \
    "$OPENSSL_SOURCE_URL" \
    "https://www.openssl.org/source/${OPENSSL_ARCHIVE_NAME}" \
    "https://www.openssl-library.org/source/old/${OPENSSL_SERIES}/${OPENSSL_ARCHIVE_NAME}" \
    "https://www.openssl.org/source/old/${OPENSSL_SERIES}/${OPENSSL_ARCHIVE_NAME}"
  extract_source_tree "$OPENSSL_SOURCE_ARCHIVE" "$source_dir"

  rm -rf "$install_dir"
  mkdir -p "$install_dir"

  echo "Building Android OpenSSL prebuilt for $abi"

  (
    cd "$source_dir"
    export ANDROID_NDK_HOME="$ANDROID_NDK_ROOT"
    export ANDROID_NDK_ROOT
    export PATH="$TOOLCHAIN_BIN_DIR:$PATH"
    export CC="$cc"
    export CXX="$cxx"
    export AR="$TOOLCHAIN_BIN_DIR/llvm-ar"
    export AS="$cc"
    export RANLIB="$TOOLCHAIN_BIN_DIR/llvm-ranlib"
    export STRIP="$TOOLCHAIN_BIN_DIR/llvm-strip"

    perl ./Configure \
      "$openssl_target" \
      shared \
      no-tests \
      no-unit-test \
      --prefix="$install_dir" \
      --openssldir="$install_dir/ssl"

    make -j"$MAKE_JOBS"
    make install_sw
  )
}

build_android_libxml2_prebuilt() {
  local abi="$1"
  local toolchain_target="$2"
  local install_dir="$LIBXML2_ANDROID_DIR/$abi"
  local source_dir="$temp_dir/libxml2-$abi-source"
  local build_dir="$temp_dir/libxml2-$abi-build"
  local cc="$TOOLCHAIN_BIN_DIR/${toolchain_target}${ANDROID_API_LEVEL}-clang"
  local cxx="$TOOLCHAIN_BIN_DIR/${toolchain_target}${ANDROID_API_LEVEL}-clang++"

  ensure_command make
  download_if_missing "$LIBXML2_SOURCE_ARCHIVE" "$LIBXML2_SOURCE_URL"
  extract_source_tree "$LIBXML2_SOURCE_ARCHIVE" "$source_dir"

  rm -rf "$install_dir" "$build_dir"
  mkdir -p "$install_dir" "$build_dir"

  echo "Building Android libxml2 prebuilt for $abi"

  (
    cd "$build_dir"
    export CC="$cc"
    export CXX="$cxx"
    export AR="$TOOLCHAIN_BIN_DIR/llvm-ar"
    export RANLIB="$TOOLCHAIN_BIN_DIR/llvm-ranlib"
    export STRIP="$TOOLCHAIN_BIN_DIR/llvm-strip"
    export PKG_CONFIG_ALLOW_CROSS=1
    export CPPFLAGS="-I$SYSROOT/usr/include"
    export LDFLAGS="-L$SYSROOT/usr/lib"

    "$source_dir/configure" \
      --host="$toolchain_target" \
      --prefix="$install_dir" \
      --enable-shared \
      --disable-static \
      --without-python \
      --without-lzma \
      --without-icu \
      --without-ftp \
      --without-http \
      --without-legacy \
      --without-docbook \
      --without-html

    make -j"$MAKE_JOBS"
    make install
  )
}

ensure_android_openssl_prebuilt() {
  local abi="$1"
  local toolchain_target="$2"
  local openssl_target="$3"
  local openssl_dir="$OPENSSL_ANDROID_DIR/$abi"

  if [[ -d "$openssl_dir/lib/pkgconfig" ]]; then
    return 0
  fi

  echo "Android OpenSSL prebuilt for ABI $abi is missing locally; restoring from cached archive or official source."
  build_android_openssl_prebuilt "$abi" "$openssl_target" "$toolchain_target"
  ensure_dependency_dir "$openssl_dir/lib/pkgconfig" "Failed to provision Android OpenSSL prebuilt for ABI $abi: $openssl_dir"
}

ensure_android_libxml2_prebuilt() {
  local abi="$1"
  local toolchain_target="$2"
  local libxml2_dir="$LIBXML2_ANDROID_DIR/$abi"

  if [[ -d "$libxml2_dir/lib/pkgconfig" ]]; then
    return 0
  fi

  echo "Android libxml2 prebuilt for ABI $abi is missing locally; restoring from cached archive or official source."
  build_android_libxml2_prebuilt "$abi" "$toolchain_target"
  ensure_dependency_dir "$libxml2_dir/lib/pkgconfig" "Failed to provision Android libxml2 prebuilt for ABI $abi: $libxml2_dir"
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
  openssl_target="$(map_abi_to_openssl_target "$abi")"
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

  ensure_android_openssl_prebuilt "$abi" "$toolchain_target" "$openssl_target"
  pkg_config_paths+=("$openssl_dir/lib/pkgconfig")
  extra_cflags+=("-I$openssl_dir/include")
  extra_ldflags+=("-L$openssl_dir/lib")
  configure_args+=(--enable-openssl --enable-version3)

  if [[ "$ENABLE_DASH" == "1" ]]; then
    ensure_android_libxml2_prebuilt "$abi" "$toolchain_target"
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
