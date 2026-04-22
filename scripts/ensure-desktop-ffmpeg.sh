#!/usr/bin/env bash
set -euo pipefail

# 这里只负责“兜底补齐”仓库约定路径，不覆盖系统优先级判断。
# 版本默认跟随 workspace 里的 ffmpeg-next 主次版本，避免把脚本锁死在某个固定发行号。
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
INSTALL_DIR="${VESPER_DESKTOP_FFMPEG_DIR:-$ROOT_DIR/third_party/ffmpeg/desktop}"
PKGCONFIG_DIR="$INSTALL_DIR/lib/pkgconfig"
PKGCONFIG_FILE="$PKGCONFIG_DIR/libavutil.pc"

if [[ -f "$PKGCONFIG_FILE" ]]; then
  printf '%s\n' "$INSTALL_DIR"
  exit 0
fi

resolve_ffmpeg_version() {
  if [[ -n "${VESPER_DESKTOP_FFMPEG_VERSION:-}" ]]; then
    printf '%s\n' "$VESPER_DESKTOP_FFMPEG_VERSION"
    return 0
  fi

  local cargo_toml="$ROOT_DIR/Cargo.toml"
  local version_line
  version_line="$(sed -n 's/^[[:space:]]*ffmpeg-next[[:space:]]*=[[:space:]]*{[[:space:]]*version[[:space:]]*=[[:space:]]*"\([^"]*\)".*$/\1/p' "$cargo_toml" | head -n 1)"
  if [[ -z "$version_line" ]]; then
    echo "Could not resolve ffmpeg-next version from $cargo_toml" >&2
    exit 1
  fi

  awk -F. '{ print $1 "." $2 }' <<<"$version_line"
}

FFMPEG_VERSION="$(resolve_ffmpeg_version)"
FFMPEG_ARCHIVE_NAME="ffmpeg-${FFMPEG_VERSION}.tar.xz"
FFMPEG_SOURCE_ARCHIVE="${VESPER_DESKTOP_FFMPEG_SOURCE_ARCHIVE:-$ROOT_DIR/$FFMPEG_ARCHIVE_NAME}"
FFMPEG_SOURCE_URL="${VESPER_DESKTOP_FFMPEG_SOURCE_URL:-https://ffmpeg.org/releases/${FFMPEG_ARCHIVE_NAME}}"

download_if_missing() {
  local archive_path="$1"
  local archive_url="$2"

  if [[ -f "$archive_path" ]]; then
    return 0
  fi

  mkdir -p "$(dirname "$archive_path")"
  echo "Downloading desktop FFmpeg source archive: $archive_url" >&2
  curl -fsSL "$archive_url" -o "$archive_path"
}

build_ffmpeg() {
  local source_archive="$1"
  local install_dir="$2"
  local temp_dir
  local source_dir
  local sdk_path
  local clang_path
  local make_jobs

  temp_dir="$(mktemp -d)"
  trap '[[ -n "${temp_dir:-}" ]] && rm -rf "$temp_dir"' EXIT

  tar -xf "$source_archive" -C "$temp_dir"
  source_dir="$(find "$temp_dir" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
  if [[ -z "$source_dir" || ! -f "$source_dir/configure" ]]; then
    echo "FFmpeg source archive did not unpack into a valid source tree: $source_archive" >&2
    exit 1
  fi

  mkdir -p "$install_dir"
  sdk_path="$(xcrun --sdk macosx --show-sdk-path)"
  clang_path="$(xcrun --sdk macosx -f clang)"
  make_jobs="$(sysctl -n hw.logicalcpu 2>/dev/null || echo 8)"

  (
    cd "$source_dir"
    ./configure \
      --prefix="$install_dir" \
      --cc="$clang_path" \
      --host-cc="$clang_path" \
      --extra-cflags="-isysroot $sdk_path -mmacosx-version-min=11.0 -w" \
      --extra-ldflags="-isysroot $sdk_path -mmacosx-version-min=11.0" \
      --host-cflags="-isysroot $sdk_path -mmacosx-version-min=11.0 -w" \
      --host-ldflags="-isysroot $sdk_path -mmacosx-version-min=11.0" \
      --disable-autodetect \
      --disable-programs \
      --disable-doc \
      --disable-debug \
      --enable-static \
      --disable-shared \
      --enable-pic
    make -j"$make_jobs"
    make install
  )
}

download_if_missing "$FFMPEG_SOURCE_ARCHIVE" "$FFMPEG_SOURCE_URL"
build_ffmpeg "$FFMPEG_SOURCE_ARCHIVE" "$INSTALL_DIR"

if [[ ! -f "$PKGCONFIG_FILE" ]]; then
  echo "Desktop FFmpeg installation completed without $PKGCONFIG_FILE" >&2
  exit 1
fi

printf '%s\n' "$INSTALL_DIR"
