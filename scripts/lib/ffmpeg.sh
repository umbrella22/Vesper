if [[ -n "${VESPER_FFMPEG_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_FFMPEG_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

vesper_ffmpeg_archive_name() {
  local version="$1"

  printf 'ffmpeg-%s.tar.xz\n' "$version"
}

vesper_ffmpeg_release_url() {
  local archive_name="$1"

  printf 'https://ffmpeg.org/releases/%s\n' "$archive_name"
}
