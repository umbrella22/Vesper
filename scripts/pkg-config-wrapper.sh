#!/usr/bin/env bash
set -euo pipefail

# macOS 桌面 FFmpeg 发现顺序固定为：
# 1. 仓库约定路径 third_party/ffmpeg/desktop
# 2. 系统当前最新的 Homebrew / pkg-config FFmpeg
# 3. 仍不存在时再触发本地安装脚本补齐约定路径
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
LOCAL_FFMPEG_DIR="${VESPER_DESKTOP_FFMPEG_DIR:-$ROOT_DIR/third_party/ffmpeg/desktop}"
LOCAL_PKGCONFIG_DIR="$LOCAL_FFMPEG_DIR/lib/pkgconfig"

resolve_real_pkg_config() {
  if [[ -n "${VESPER_REAL_PKG_CONFIG:-}" ]]; then
    printf '%s\n' "$VESPER_REAL_PKG_CONFIG"
    return 0
  fi

  if command -v pkg-config >/dev/null 2>&1; then
    command -v pkg-config
    return 0
  fi

  if command -v pkgconf >/dev/null 2>&1; then
    command -v pkgconf
    return 0
  fi

  echo "pkg-config-wrapper could not locate a real pkg-config executable" >&2
  exit 1
}

contains_ffmpeg_package() {
  local arg
  for arg in "$@"; do
    case "$arg" in
      libavutil|libavcodec|libavformat|libavfilter|libswresample|libswscale|libavdevice|libavresample|libpostproc)
        return 0
        ;;
    esac
  done
  return 1
}

prepend_pkg_config_path() {
  local candidate="$1"
  if [[ -n "${PKG_CONFIG_PATH:-}" ]]; then
    printf '%s:%s\n' "$candidate" "$PKG_CONFIG_PATH"
  else
    printf '%s\n' "$candidate"
  fi
}

exec_with_pkg_config_path() {
  local real_pkg_config="$1"
  local pkg_config_path="$2"
  shift 2
  export PKG_CONFIG_PATH
  PKG_CONFIG_PATH="$(prepend_pkg_config_path "$pkg_config_path")"
  exec "$real_pkg_config" "$@"
}

REAL_PKG_CONFIG="$(resolve_real_pkg_config)"

if [[ "${CARGO_CFG_TARGET_OS:-}" == "macos" ]] && contains_ffmpeg_package "$@"; then
  if [[ -f "$LOCAL_PKGCONFIG_DIR/libavutil.pc" ]]; then
    exec_with_pkg_config_path "$REAL_PKG_CONFIG" "$LOCAL_PKGCONFIG_DIR" "$@"
  fi

  if command -v brew >/dev/null 2>&1; then
    BREW_FFMPEG_PREFIX="$(brew --prefix ffmpeg 2>/dev/null || true)"
    if [[ -n "$BREW_FFMPEG_PREFIX" && -f "$BREW_FFMPEG_PREFIX/lib/pkgconfig/libavutil.pc" ]]; then
      exec_with_pkg_config_path "$REAL_PKG_CONFIG" "$BREW_FFMPEG_PREFIX/lib/pkgconfig" "$@"
    fi
  fi

  if "$REAL_PKG_CONFIG" "$@" >/dev/null 2>&1; then
    exec "$REAL_PKG_CONFIG" "$@"
  fi

  "$ROOT_DIR/scripts/ensure-desktop-ffmpeg.sh" >/dev/null
  exec_with_pkg_config_path "$REAL_PKG_CONFIG" "$LOCAL_PKGCONFIG_DIR" "$@"
fi

exec "$REAL_PKG_CONFIG" "$@"
