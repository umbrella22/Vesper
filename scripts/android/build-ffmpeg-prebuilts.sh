#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/android.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
ANDROID_SDK_ROOT="$(vesper_android_sdk_root)"
ANDROID_NDK_VERSION="$(vesper_android_ndk_version)"
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-}"
ANDROID_API_LEVEL="${VESPER_ANDROID_FFMPEG_ANDROID_API:-26}"
FFMPEG_VERSION="${VESPER_ANDROID_FFMPEG_VERSION:-8.1}"
FFMPEG_ARCHIVE_NAME="$(vesper_ffmpeg_archive_name "$FFMPEG_VERSION")"
FFMPEG_SOURCE_URL="${VESPER_ANDROID_FFMPEG_SOURCE_URL:-$(vesper_ffmpeg_release_url "$FFMPEG_ARCHIVE_NAME")}"
FFMPEG_SOURCE_ARCHIVE="${VESPER_ANDROID_FFMPEG_SOURCE_ARCHIVE:-$ROOT_DIR/${FFMPEG_ARCHIVE_NAME}}"
FFMPEG_OUTPUT_DIR="${VESPER_ANDROID_FFMPEG_OUTPUT_DIR:-$ROOT_DIR/third_party/ffmpeg/android}"
OPENSSL_VERSION="${VESPER_ANDROID_OPENSSL_VERSION:-3.6.2}"
OPENSSL_SERIES="${OPENSSL_VERSION%.*}"
OPENSSL_ARCHIVE_NAME="openssl-${OPENSSL_VERSION}.tar.gz"
OPENSSL_SOURCE_URL="${VESPER_ANDROID_OPENSSL_SOURCE_URL:-https://www.openssl.org/source/${OPENSSL_ARCHIVE_NAME}}"
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

ensure_dependency_dir() {
  local path="$1"
  local message="$2"

  if [[ ! -d "$path" ]]; then
    echo "$message" >&2
    exit 1
  fi
}

build_android_openssl_prebuilt() {
  local abi="$1"
  local openssl_target="$2"
  local toolchain_target="$3"
  local install_dir="$OPENSSL_ANDROID_DIR/$abi"
  local source_dir="$temp_dir/openssl-$abi"
  local cc="$TOOLCHAIN_BIN_DIR/${toolchain_target}${ANDROID_API_LEVEL}-clang"
  local cxx="$TOOLCHAIN_BIN_DIR/${toolchain_target}${ANDROID_API_LEVEL}-clang++"

  vesper_require_command perl
  vesper_require_command make
  vesper_download_if_missing \
    "$OPENSSL_SOURCE_ARCHIVE" \
    "$OPENSSL_SOURCE_URL" \
    "https://www.openssl.org/source/old/${OPENSSL_SERIES}/${OPENSSL_ARCHIVE_NAME}" \
    "https://www.openssl-library.org/source/${OPENSSL_ARCHIVE_NAME}" \
    "https://www.openssl-library.org/source/old/${OPENSSL_SERIES}/${OPENSSL_ARCHIVE_NAME}"
  vesper_extract_source_tree "$OPENSSL_SOURCE_ARCHIVE" "$source_dir"

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

  vesper_require_command make
  vesper_download_if_missing "$LIBXML2_SOURCE_ARCHIVE" "$LIBXML2_SOURCE_URL"
  vesper_extract_source_tree "$LIBXML2_SOURCE_ARCHIVE" "$source_dir"

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
      --without-iconv \
      --without-python \
      --without-lzma \
      --without-icu \
      --without-http \
      --without-legacy \
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
done < <(vesper_android_resolve_selected_abis "$@")

required_targets=()
for abi in "${selected_abis[@]}"; do
  required_targets+=("$(vesper_android_abi_to_rust_target "$abi")")
done

vesper_android_require_rust_targets "${required_targets[@]}"

if ! ANDROID_NDK_ROOT="$(vesper_android_resolve_ndk_root "$ANDROID_SDK_ROOT" "$ANDROID_NDK_ROOT" "$ANDROID_NDK_VERSION")"; then
  vesper_android_report_missing_ndk "$ANDROID_SDK_ROOT" "$ANDROID_NDK_VERSION"
  exit 1
fi

HOST_TAG="$(vesper_android_resolve_host_tag "$ANDROID_NDK_ROOT")"
TOOLCHAIN_ROOT="$ANDROID_NDK_ROOT/toolchains/llvm/prebuilt/$HOST_TAG"
TOOLCHAIN_BIN_DIR="$TOOLCHAIN_ROOT/bin"
SYSROOT="$TOOLCHAIN_ROOT/sysroot"
MAKE_JOBS="$(vesper_make_jobs)"

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

vesper_download_if_missing "$FFMPEG_SOURCE_ARCHIVE" "$FFMPEG_SOURCE_URL"

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
  ffmpeg_arch="$(vesper_android_abi_to_ffmpeg_arch "$abi")"
  ffmpeg_cpu="$(vesper_android_abi_to_ffmpeg_cpu "$abi")"
  toolchain_target="$(vesper_android_abi_to_rust_target "$abi")"
  openssl_target="$(vesper_android_abi_to_openssl_target "$abi")"
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
