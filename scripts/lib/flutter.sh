if [[ -n "${VESPER_FLUTTER_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_FLUTTER_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

VESPER_FLUTTER_CORE_PACKAGES=(
  vesper_player_platform_interface
  vesper_player_android
  vesper_player_ios
  vesper_player
  vesper_player_external_playback
  vesper_player_ui
)

VESPER_FLUTTER_OPTIONAL_PLUGIN_PACKAGES=(
  vesper_player_source_normalizer_ffmpeg
)

VESPER_FLUTTER_PACKAGES=("${VESPER_FLUTTER_CORE_PACKAGES[@]}")
case "${VESPER_FLUTTER_INCLUDE_OPTIONAL_PLUGINS:-0}" in
  1|true|TRUE|yes|YES)
    VESPER_FLUTTER_PACKAGES+=("${VESPER_FLUTTER_OPTIONAL_PLUGIN_PACKAGES[@]}")
    ;;
esac
