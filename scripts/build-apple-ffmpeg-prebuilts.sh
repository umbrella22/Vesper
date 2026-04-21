#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
FFMPEG_VERSION="${VESPER_APPLE_FFMPEG_VERSION:-8.1}"
FFMPEG_ARCHIVE_NAME="ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_SOURCE_URL="${VESPER_APPLE_FFMPEG_SOURCE_URL:-https://ffmpeg.org/releases/${FFMPEG_ARCHIVE_NAME}}"
FFMPEG_SOURCE_ARCHIVE="${VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE:-$ROOT_DIR/${FFMPEG_ARCHIVE_NAME}}"
FFMPEG_OUTPUT_DIR="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-$ROOT_DIR/third_party/ffmpeg/apple}"
IOS_DEPLOYMENT_TARGET="${VESPER_APPLE_IOS_DEPLOYMENT_TARGET:-17.0}"
ENABLE_DASH="${VESPER_APPLE_FFMPEG_ENABLE_DASH:-1}"
# Apple 侧预编译 FFmpeg slice 统一收敛为 arm64-only。
DEFAULT_SLICES=(
  "ios-arm64"
  "ios-simulator-arm64"
)

resolve_selected_slices() {
  local -a resolved=()
  local token

  if [[ $# -gt 0 ]]; then
    resolved=("$@")
  else
    resolved=("${DEFAULT_SLICES[@]}")
  fi

  if [[ ${#resolved[@]} -eq 0 ]]; then
    echo "No Apple FFmpeg slices were selected." >&2
    exit 1
  fi

  for token in "${resolved[@]}"; do
    case "$token" in
      ios-arm64|ios-simulator-arm64)
        ;;
      *)
        echo "Unsupported Apple FFmpeg slice: $token" >&2
        echo "Supported slices: ios-arm64, ios-simulator-arm64" >&2
        exit 1
        ;;
    esac
  done

  printf '%s\n' "${resolved[@]}"
}

slice_sdk() {
  case "$1" in
    ios-arm64)
      echo "iphoneos"
      ;;
    ios-simulator-arm64)
      echo "iphonesimulator"
      ;;
    *)
      return 1
      ;;
  esac
}

slice_arch() {
  case "$1" in
    ios-arm64|ios-simulator-arm64)
      echo "arm64"
      ;;
    *)
      return 1
      ;;
  esac
}

slice_target_triple() {
  case "$1" in
    ios-arm64)
      echo "arm64-apple-ios${IOS_DEPLOYMENT_TARGET}"
      ;;
    ios-simulator-arm64)
      echo "arm64-apple-ios${IOS_DEPLOYMENT_TARGET}-simulator"
      ;;
    *)
      return 1
      ;;
  esac
}

slice_output_root() {
  case "$1" in
    ios-arm64)
      echo "$FFMPEG_OUTPUT_DIR/ios"
      ;;
    ios-simulator-arm64)
      echo "$FFMPEG_OUTPUT_DIR/ios-simulator"
      ;;
    *)
      return 1
      ;;
  esac
}

slice_output_libdir() {
  case "$1" in
    ios-arm64|ios-simulator-arm64)
      echo "arm64"
      ;;
    *)
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

ensure_command() {
  local command_name="$1"

  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "Missing required command: $command_name" >&2
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

extract_libxml2_version() {
  local sdk_path="$1"
  local header_path="$sdk_path/usr/include/libxml2/libxml/xmlversion.h"

  if [[ ! -f "$header_path" ]]; then
    echo "2.0.0"
    return 0
  fi

  sed -n 's/^#define LIBXML_DOTTED_VERSION "\(.*\)"$/\1/p' "$header_path" | head -n 1
}

selected_slices=()
while IFS= read -r slice; do
  selected_slices+=("$slice")
done < <(resolve_selected_slices "$@")

ensure_command tar
ensure_command make
ensure_command xcrun

download_if_missing "$FFMPEG_SOURCE_ARCHIVE" "$FFMPEG_SOURCE_URL"

MAKE_JOBS="$(resolve_make_jobs)"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vesper-apple-ffmpeg.XXXXXX")"
cleanup() {
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

mkdir -p "$FFMPEG_OUTPUT_DIR"

for slice in "${selected_slices[@]}"; do
  sdk_name="$(slice_sdk "$slice")"
  arch="$(slice_arch "$slice")"
  clang_target="$(slice_target_triple "$slice")"
  output_root="$(slice_output_root "$slice")"
  output_libdir="$(slice_output_libdir "$slice")"
  sdk_path="$(xcrun --sdk "$sdk_name" --show-sdk-path)"
  cc_path="$(xcrun --sdk "$sdk_name" -f clang)"
  libxml2_version="$(extract_libxml2_version "$sdk_path")"
  source_dir="$WORK_DIR/source-$slice"
  install_dir="$WORK_DIR/install-$slice"
  pkgconfig_dir="$WORK_DIR/pkgconfig-$slice"

  rm -rf "$source_dir" "$install_dir" "$pkgconfig_dir"
  mkdir -p "$source_dir" "$install_dir" "$pkgconfig_dir"
  tar -xf "$FFMPEG_SOURCE_ARCHIVE" -C "$source_dir" --strip-components=1

  cat > "$pkgconfig_dir/libxml-2.0.pc" <<EOF
prefix=$sdk_path/usr
exec_prefix=\${prefix}
libdir=$sdk_path/usr/lib
includedir=$sdk_path/usr/include

Name: libxml2
Description: Apple SDK libxml2
Version: ${libxml2_version:-2.0.0}
Libs: -L\${libdir} -lxml2 -lz
Cflags: -I\${includedir}/libxml2
EOF

  extra_cflags=(
    "-target $clang_target"
    "-isysroot $sdk_path"
    "-fPIC"
    "-I$sdk_path/usr/include"
  )
  extra_ldflags=(
    "-target $clang_target"
    "-isysroot $sdk_path"
    "-L$sdk_path/usr/lib"
    "-lz"
  )

  configure_args=(
    "--prefix=$install_dir"
    "--install-name-dir=@rpath"
    "--enable-cross-compile"
    "--target-os=darwin"
    "--arch=$arch"
    "--cc=$cc_path"
    "--sysroot=$sdk_path"
    "--disable-programs"
    "--disable-doc"
    "--disable-autodetect"
    "--enable-static"
    "--enable-shared"
    "--enable-pic"
    "--enable-network"
    "--enable-securetransport"
    "--extra-cflags=${extra_cflags[*]}"
    "--extra-ldflags=${extra_ldflags[*]}"
  )

  if [[ "$ENABLE_DASH" == "1" ]]; then
    configure_args+=("--enable-libxml2")
  fi

  if [[ "$arch" == "x86_64" ]]; then
    # iOS simulator x86_64 在 Apple Silicon 宿主上更容易被内联汇编绊住。
    configure_args+=("--disable-asm")
  fi

  echo
  echo "Building Apple FFmpeg prebuilt for $slice"
  (
    cd "$source_dir"
    env \
      PKG_CONFIG_ALLOW_CROSS=1 \
      PKG_CONFIG_PATH="$pkgconfig_dir" \
      PKG_CONFIG_LIBDIR="$pkgconfig_dir" \
      ./configure "${configure_args[@]}"
    make -j"$MAKE_JOBS"
    make install
  )

  mkdir -p "$output_root/lib/$output_libdir"
  rm -rf "$output_root/lib/$output_libdir"
  mkdir -p "$output_root/lib/$output_libdir"
  cp "$install_dir"/lib/*.a "$output_root/lib/$output_libdir/"
  if compgen -G "$install_dir/lib/"'lib*.dylib*' >/dev/null; then
    cp -RP "$install_dir"/lib/lib*.dylib* "$output_root/lib/$output_libdir/"
  fi

  rm -rf "$output_root/include"
  cp -R "$install_dir/include" "$output_root/include"
done

echo
echo "Built Apple FFmpeg prebuilts into:"
echo "  $FFMPEG_OUTPUT_DIR"
echo "Selected slices:"
for slice in "${selected_slices[@]}"; do
  echo "  $slice"
done
