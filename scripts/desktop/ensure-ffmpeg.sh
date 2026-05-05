#!/usr/bin/env bash
set -euo pipefail

# This script only fills the repository-local fallback path and does not change
# the system-first resolution order.
# The default version follows the workspace ffmpeg-next major/minor version so
# the helper is not pinned to a fixed FFmpeg release.
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
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
FFMPEG_ARCHIVE_NAME="$(vesper_ffmpeg_archive_name "$FFMPEG_VERSION")"
FFMPEG_SOURCE_ARCHIVE="${VESPER_DESKTOP_FFMPEG_SOURCE_ARCHIVE:-$ROOT_DIR/$FFMPEG_ARCHIVE_NAME}"
FFMPEG_SOURCE_URL="${VESPER_DESKTOP_FFMPEG_SOURCE_URL:-$(vesper_ffmpeg_release_url "$FFMPEG_ARCHIVE_NAME")}"

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
  make_jobs="$(vesper_make_jobs)"

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

vesper_download_if_missing "$FFMPEG_SOURCE_ARCHIVE" "$FFMPEG_SOURCE_URL"
build_ffmpeg "$FFMPEG_SOURCE_ARCHIVE" "$INSTALL_DIR"

if [[ ! -f "$PKGCONFIG_FILE" ]]; then
  echo "Desktop FFmpeg installation completed without $PKGCONFIG_FILE" >&2
  exit 1
fi

printf '%s\n' "$INSTALL_DIR"
