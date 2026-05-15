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

vesper_ffmpeg_shell_quote() {
  local value="$1"

  printf '%q' "$value"
}

vesper_ffmpeg_join_quoted() {
  local separator=""
  local value

  for value in "$@"; do
    printf '%s' "$separator"
    vesper_ffmpeg_shell_quote "$value"
    separator=" "
  done
}

vesper_ffmpeg_join_csv() {
  local separator=""
  local value

  for value in "$@"; do
    printf '%s%s' "$separator" "$value"
    separator=","
  done
}

vesper_ffmpeg_hash_text() {
  local value="$1"

  if command -v shasum >/dev/null 2>&1; then
    printf '%s' "$value" | shasum -a 256 | awk '{print substr($1, 1, 12)}'
  else
    printf '%s' "$value" | cksum | awk '{print $1}'
  fi
}

vesper_ffmpeg_uppercase() {
  printf '%s' "$1" | tr '[:lower:]' '[:upper:]'
}

vesper_ffmpeg_env_value() {
  local platform_key="$1"
  local suffix="$2"
  local default_value="${3:-}"
  local specific_name="VESPER_${platform_key}_FFMPEG_${suffix}"
  local generic_name="VESPER_FFMPEG_${suffix}"

  if [[ -n "${!specific_name:-}" ]]; then
    printf '%s\n' "${!specific_name}"
  elif [[ -n "${!generic_name:-}" ]]; then
    printf '%s\n' "${!generic_name}"
  else
    printf '%s\n' "$default_value"
  fi
}

vesper_ffmpeg_append_list() {
  local target="$1"
  local value="$2"
  local token

  value="${value//,/ }"
  for token in $value; do
    [[ -n "$token" ]] || continue
    eval "$target+=(\"\$token\")"
  done
}

vesper_ffmpeg_append_env_list() {
  local platform_key="$1"
  local suffix="$2"
  local target="$3"
  local generic_name="VESPER_FFMPEG_${suffix}"
  local specific_name="VESPER_${platform_key}_FFMPEG_${suffix}"

  if [[ -n "${!generic_name:-}" ]]; then
    vesper_ffmpeg_append_list "$target" "${!generic_name}"
    VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
  fi
  if [[ -n "${!specific_name:-}" ]]; then
    vesper_ffmpeg_append_list "$target" "${!specific_name}"
    VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
  fi
}

vesper_ffmpeg_append_env_words() {
  local platform_key="$1"
  local suffix="$2"
  local target="$3"
  local generic_name="VESPER_FFMPEG_${suffix}"
  local specific_name="VESPER_${platform_key}_FFMPEG_${suffix}"
  local token

  if [[ -n "${!generic_name:-}" ]]; then
    for token in ${!generic_name}; do
      eval "$target+=(\"\$token\")"
    done
    VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
  fi
  if [[ -n "${!specific_name:-}" ]]; then
    for token in ${!specific_name}; do
      eval "$target+=(\"\$token\")"
    done
    VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
  fi
}

vesper_ffmpeg_reset_options() {
  local platform_key="$1"
  local tls_generic_name="VESPER_FFMPEG_TLS_BACKEND"
  local tls_specific_name="VESPER_${platform_key}_FFMPEG_TLS_BACKEND"
  local dash_generic_name="VESPER_FFMPEG_ENABLE_DASH"
  local dash_specific_name="VESPER_${platform_key}_FFMPEG_ENABLE_DASH"

  VESPER_FFMPEG_PROFILE="$(vesper_ffmpeg_env_value "$platform_key" PROFILE legacy)"
  VESPER_FFMPEG_TLS_BACKEND="$(vesper_ffmpeg_env_value "$platform_key" TLS_BACKEND "")"
  VESPER_FFMPEG_ENABLE_DASH="$(vesper_ffmpeg_env_value "$platform_key" ENABLE_DASH "")"
  VESPER_FFMPEG_ENABLE_DASH_EXPLICIT=0
  VESPER_FFMPEG_FORCE="$(vesper_ffmpeg_env_value "$platform_key" FORCE 0)"
  VESPER_FFMPEG_ACK_GPL_NONFREE="$(vesper_ffmpeg_env_value "$platform_key" ACKNOWLEDGE_GPL_NONFREE 0)"
  VESPER_FFMPEG_OVERLAY_COUNT=0
  VESPER_FFMPEG_ENABLE_LIBRARIES=()
  VESPER_FFMPEG_ENABLE_DEMUXERS=()
  VESPER_FFMPEG_ENABLE_MUXERS=()
  VESPER_FFMPEG_ENABLE_PROTOCOLS=()
  VESPER_FFMPEG_ENABLE_PARSERS=()
  VESPER_FFMPEG_ENABLE_BSFS=()
  VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS=()
  VESPER_FFMPEG_POSITIONAL_ARGS=()
  VESPER_FFMPEG_CONFIGURE_ARGS=()
  VESPER_FFMPEG_FINAL_LIBRARIES=()
  VESPER_FFMPEG_FINAL_DEMUXERS=()
  VESPER_FFMPEG_FINAL_MUXERS=()
  VESPER_FFMPEG_FINAL_PROTOCOLS=()
  VESPER_FFMPEG_FINAL_PARSERS=()
  VESPER_FFMPEG_FINAL_BSFS=()
  VESPER_FFMPEG_USE_OPENSSL=0
  VESPER_FFMPEG_USE_LIBXML2=0
  VESPER_FFMPEG_LICENSE_FLAGS=()
  VESPER_FFMPEG_EXTERNAL_DEPS=()

  if [[ -n "$VESPER_FFMPEG_ENABLE_DASH" ]]; then
    VESPER_FFMPEG_ENABLE_DASH_EXPLICIT=1
  fi
  if [[ -n "${!tls_generic_name:-}" || -n "${!tls_specific_name:-}" ]]; then
    VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
  fi
  if [[ -n "${!dash_generic_name:-}" || -n "${!dash_specific_name:-}" ]]; then
    VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
  fi

  vesper_ffmpeg_append_env_list "$platform_key" ENABLE_LIBRARIES VESPER_FFMPEG_ENABLE_LIBRARIES
  vesper_ffmpeg_append_env_list "$platform_key" ENABLE_DEMUXERS VESPER_FFMPEG_ENABLE_DEMUXERS
  vesper_ffmpeg_append_env_list "$platform_key" ENABLE_MUXERS VESPER_FFMPEG_ENABLE_MUXERS
  vesper_ffmpeg_append_env_list "$platform_key" ENABLE_PROTOCOLS VESPER_FFMPEG_ENABLE_PROTOCOLS
  vesper_ffmpeg_append_env_list "$platform_key" ENABLE_PARSERS VESPER_FFMPEG_ENABLE_PARSERS
  vesper_ffmpeg_append_env_list "$platform_key" ENABLE_BSFS VESPER_FFMPEG_ENABLE_BSFS
  vesper_ffmpeg_append_env_words "$platform_key" EXTRA_CONFIGURE_ARGS VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS
}

vesper_ffmpeg_require_arg() {
  local option="$1"
  local value="${2:-}"

  if [[ -z "$value" ]]; then
    echo "$option requires a value." >&2
    exit 1
  fi
}

vesper_ffmpeg_parse_common_args() {
  local platform="$1"
  local platform_key
  platform_key="$(vesper_ffmpeg_uppercase "$platform")"
  shift

  vesper_ffmpeg_reset_options "$platform_key"

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --ffmpeg-profile|--profile)
        vesper_ffmpeg_require_arg "$1" "${2:-}"
        VESPER_FFMPEG_PROFILE="$2"
        shift 2
        ;;
      --ffmpeg-profile=*|--profile=*)
        VESPER_FFMPEG_PROFILE="${1#*=}"
        shift
        ;;
      --enable-libraries)
        vesper_ffmpeg_require_arg "$1" "${2:-}"
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_LIBRARIES "$2"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift 2
        ;;
      --enable-libraries=*)
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_LIBRARIES "${1#*=}"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --enable-demuxers)
        vesper_ffmpeg_require_arg "$1" "${2:-}"
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_DEMUXERS "$2"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift 2
        ;;
      --enable-demuxers=*)
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_DEMUXERS "${1#*=}"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --enable-muxers)
        vesper_ffmpeg_require_arg "$1" "${2:-}"
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_MUXERS "$2"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift 2
        ;;
      --enable-muxers=*)
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_MUXERS "${1#*=}"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --enable-protocols)
        vesper_ffmpeg_require_arg "$1" "${2:-}"
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_PROTOCOLS "$2"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift 2
        ;;
      --enable-protocols=*)
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_PROTOCOLS "${1#*=}"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --enable-parsers)
        vesper_ffmpeg_require_arg "$1" "${2:-}"
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_PARSERS "$2"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift 2
        ;;
      --enable-parsers=*)
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_PARSERS "${1#*=}"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --enable-bsfs)
        vesper_ffmpeg_require_arg "$1" "${2:-}"
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_BSFS "$2"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift 2
        ;;
      --enable-bsfs=*)
        vesper_ffmpeg_append_list VESPER_FFMPEG_ENABLE_BSFS "${1#*=}"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --extra-configure-arg)
        vesper_ffmpeg_require_arg "$1" "${2:-}"
        VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS+=("$2")
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift 2
        ;;
      --extra-configure-arg=*)
        VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS+=("${1#*=}")
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --tls-backend)
        vesper_ffmpeg_require_arg "$1" "${2:-}"
        VESPER_FFMPEG_TLS_BACKEND="$2"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift 2
        ;;
      --tls-backend=*)
        VESPER_FFMPEG_TLS_BACKEND="${1#*=}"
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --enable-dash)
        VESPER_FFMPEG_ENABLE_DASH=1
        VESPER_FFMPEG_ENABLE_DASH_EXPLICIT=1
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --disable-dash)
        VESPER_FFMPEG_ENABLE_DASH=0
        VESPER_FFMPEG_ENABLE_DASH_EXPLICIT=1
        VESPER_FFMPEG_OVERLAY_COUNT=$((VESPER_FFMPEG_OVERLAY_COUNT + 1))
        shift
        ;;
      --force)
        VESPER_FFMPEG_FORCE=1
        shift
        ;;
      --acknowledge-gpl-nonfree)
        VESPER_FFMPEG_ACK_GPL_NONFREE=1
        shift
        ;;
      --)
        shift
        while [[ $# -gt 0 ]]; do
          VESPER_FFMPEG_POSITIONAL_ARGS+=("$1")
          shift
        done
        ;;
      --*)
        echo "Unknown FFmpeg build option: $1" >&2
        exit 1
        ;;
      *)
        VESPER_FFMPEG_POSITIONAL_ARGS+=("$1")
        shift
        ;;
    esac
  done

  vesper_ffmpeg_validate_profile "$platform"
  vesper_ffmpeg_prepare_component_args "$platform"
}

vesper_ffmpeg_array_contains() {
  local needle="$1"
  shift
  local value

  for value in "$@"; do
    if [[ "$value" == "$needle" ]]; then
      return 0
    fi
  done
  return 1
}

vesper_ffmpeg_append_unique() {
  local target="$1"
  local value="$2"
  local existing
  local restore_nounset=0

  if [[ "$-" == *u* ]]; then
    restore_nounset=1
    set +u
  fi
  eval "existing=(\"\${${target}[@]}\")"
  if ! vesper_ffmpeg_array_contains "$value" ${existing[@]+"${existing[@]}"}; then
    eval "$target+=(\"\$value\")"
  fi
  if [[ "$restore_nounset" == "1" ]]; then
    set -u
  fi
}

vesper_ffmpeg_merge_unique() {
  local target="$1"
  shift
  local value

  for value in "$@"; do
    vesper_ffmpeg_append_unique "$target" "$value"
  done
}

vesper_ffmpeg_remove_from_array() {
  local target="$1"
  local needle="$2"
  local values=()
  local kept=()
  local value
  local restore_nounset=0

  if [[ "$-" == *u* ]]; then
    restore_nounset=1
    set +u
  fi
  eval "values=(\"\${${target}[@]}\")"
  for value in ${values[@]+"${values[@]}"}; do
    if [[ "$value" != "$needle" ]]; then
      kept+=("$value")
    fi
  done
  eval "$target=()"
  for value in ${kept[@]+"${kept[@]}"}; do
    eval "$target+=(\"\$value\")"
  done
  if [[ "$restore_nounset" == "1" ]]; then
    set -u
  fi
}

vesper_ffmpeg_validate_name_list() {
  local label="$1"
  shift
  local value

  for value in "$@"; do
    if [[ ! "$value" =~ ^[A-Za-z0-9_.+-]+$ ]]; then
      echo "Invalid FFmpeg $label name: $value" >&2
      exit 1
    fi
  done
}

vesper_ffmpeg_validate_library_list() {
  local value

  for value in "$@"; do
    case "$value" in
      avcodec|avformat|avutil|avfilter|avdevice|swscale|swresample)
        ;;
      *)
        echo "Unsupported FFmpeg library name: $value" >&2
        echo "Supported libraries: avcodec, avformat, avutil, avfilter, avdevice, swscale, swresample" >&2
        exit 1
        ;;
    esac
  done
}

vesper_ffmpeg_validate_profile() {
  local platform="$1"

  case "$VESPER_FFMPEG_PROFILE" in
    legacy|remux-local|custom)
      ;;
    *)
      echo "Unsupported FFmpeg profile: $VESPER_FFMPEG_PROFILE" >&2
      echo "Supported profiles: legacy, remux-local, custom" >&2
      exit 1
      ;;
  esac

  if [[ -z "$VESPER_FFMPEG_TLS_BACKEND" ]]; then
    case "$platform:$VESPER_FFMPEG_PROFILE" in
      android:legacy)
        VESPER_FFMPEG_TLS_BACKEND="openssl"
        ;;
      apple:legacy)
        VESPER_FFMPEG_TLS_BACKEND="securetransport"
        ;;
      *)
        VESPER_FFMPEG_TLS_BACKEND="none"
        ;;
    esac
  fi

  case "$platform:$VESPER_FFMPEG_TLS_BACKEND" in
    android:none|android:openssl|apple:none|apple:securetransport)
      ;;
    android:*)
      echo "Unsupported Android FFmpeg TLS backend: $VESPER_FFMPEG_TLS_BACKEND" >&2
      echo "Supported values: none, openssl" >&2
      exit 1
      ;;
    apple:*)
      echo "Unsupported Apple FFmpeg TLS backend: $VESPER_FFMPEG_TLS_BACKEND" >&2
      echo "Supported values: none, securetransport" >&2
      exit 1
      ;;
    *)
      echo "Unsupported FFmpeg platform: $platform" >&2
      exit 1
      ;;
  esac

  vesper_ffmpeg_validate_library_list ${VESPER_FFMPEG_ENABLE_LIBRARIES[@]+"${VESPER_FFMPEG_ENABLE_LIBRARIES[@]}"}
  vesper_ffmpeg_validate_name_list demuxer ${VESPER_FFMPEG_ENABLE_DEMUXERS[@]+"${VESPER_FFMPEG_ENABLE_DEMUXERS[@]}"}
  vesper_ffmpeg_validate_name_list muxer ${VESPER_FFMPEG_ENABLE_MUXERS[@]+"${VESPER_FFMPEG_ENABLE_MUXERS[@]}"}
  vesper_ffmpeg_validate_name_list protocol ${VESPER_FFMPEG_ENABLE_PROTOCOLS[@]+"${VESPER_FFMPEG_ENABLE_PROTOCOLS[@]}"}
  vesper_ffmpeg_validate_name_list parser ${VESPER_FFMPEG_ENABLE_PARSERS[@]+"${VESPER_FFMPEG_ENABLE_PARSERS[@]}"}
  vesper_ffmpeg_validate_name_list bitstream-filter ${VESPER_FFMPEG_ENABLE_BSFS[@]+"${VESPER_FFMPEG_ENABLE_BSFS[@]}"}
}

vesper_ffmpeg_protocols_need_network() {
  local protocol

  for protocol in "$@"; do
    case "$protocol" in
      async|cache|concatf|crypto|data|ffrtmpcrypt|ftp|gopher|gophers|hls|http|httpproxy|https|icecast|mmsh|mmst|rtmp|rtmpe|rtmps|rtmpt|rtmpte|rtmpts|rtp|sctp|srtp|subfile|tcp|tls|udp|unix)
        return 0
        ;;
    esac
  done

  return 1
}

vesper_ffmpeg_has_flag() {
  local flag="$1"
  shift
  local value

  for value in "$@"; do
    if [[ "$value" == "$flag" || "$value" == "$flag="* ]]; then
      return 0
    fi
  done

  return 1
}

vesper_ffmpeg_prepare_component_args() {
  local platform="$1"
  local protocol

  VESPER_FFMPEG_CONFIGURE_ARGS=()
  VESPER_FFMPEG_FINAL_LIBRARIES=()
  VESPER_FFMPEG_FINAL_DEMUXERS=()
  VESPER_FFMPEG_FINAL_MUXERS=()
  VESPER_FFMPEG_FINAL_PROTOCOLS=()
  VESPER_FFMPEG_FINAL_PARSERS=()
  VESPER_FFMPEG_FINAL_BSFS=()
  VESPER_FFMPEG_LICENSE_FLAGS=()
  VESPER_FFMPEG_EXTERNAL_DEPS=()
  VESPER_FFMPEG_USE_OPENSSL=0
  VESPER_FFMPEG_USE_LIBXML2=0

  case "$VESPER_FFMPEG_PROFILE" in
    remux-local)
      vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_LIBRARIES avcodec avformat avutil
      vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_DEMUXERS hls dash concat flv mov matroska mpegts aac
      vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_MUXERS mp4 mov matroska
      vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_PROTOCOLS file pipe
      vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_PARSERS aac ac3 av1 flac h264 hevc mpeg4video opus vp8 vp9
      vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_BSFS aac_adtstoasc extract_extradata h264_metadata hevc_metadata
      ;;
    custom|legacy)
      ;;
  esac

  vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_LIBRARIES ${VESPER_FFMPEG_ENABLE_LIBRARIES[@]+"${VESPER_FFMPEG_ENABLE_LIBRARIES[@]}"}
  vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_DEMUXERS ${VESPER_FFMPEG_ENABLE_DEMUXERS[@]+"${VESPER_FFMPEG_ENABLE_DEMUXERS[@]}"}
  vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_MUXERS ${VESPER_FFMPEG_ENABLE_MUXERS[@]+"${VESPER_FFMPEG_ENABLE_MUXERS[@]}"}
  vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_PROTOCOLS ${VESPER_FFMPEG_ENABLE_PROTOCOLS[@]+"${VESPER_FFMPEG_ENABLE_PROTOCOLS[@]}"}
  vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_PARSERS ${VESPER_FFMPEG_ENABLE_PARSERS[@]+"${VESPER_FFMPEG_ENABLE_PARSERS[@]}"}
  vesper_ffmpeg_merge_unique VESPER_FFMPEG_FINAL_BSFS ${VESPER_FFMPEG_ENABLE_BSFS[@]+"${VESPER_FFMPEG_ENABLE_BSFS[@]}"}

  if [[ -z "$VESPER_FFMPEG_ENABLE_DASH" ]]; then
    case "$VESPER_FFMPEG_PROFILE" in
      legacy|remux-local)
        VESPER_FFMPEG_ENABLE_DASH=1
        ;;
      custom)
        if vesper_ffmpeg_array_contains dash ${VESPER_FFMPEG_FINAL_DEMUXERS[@]+"${VESPER_FFMPEG_FINAL_DEMUXERS[@]}"}; then
          VESPER_FFMPEG_ENABLE_DASH=1
        else
          VESPER_FFMPEG_ENABLE_DASH=0
        fi
        ;;
    esac
  fi

  if [[ "$VESPER_FFMPEG_ENABLE_DASH" != "0" && "$VESPER_FFMPEG_ENABLE_DASH" != "1" ]]; then
    echo "FFmpeg DASH toggle must be 0 or 1, got: $VESPER_FFMPEG_ENABLE_DASH" >&2
    exit 1
  fi

  if [[ "$VESPER_FFMPEG_ENABLE_DASH" == "0" ]]; then
    vesper_ffmpeg_remove_from_array VESPER_FFMPEG_FINAL_DEMUXERS dash
  fi

  if [[ "$VESPER_FFMPEG_ENABLE_DASH" == "1" ]]; then
    VESPER_FFMPEG_USE_LIBXML2=1
    VESPER_FFMPEG_EXTERNAL_DEPS+=(libxml2)
  fi

  if [[ "$VESPER_FFMPEG_PROFILE" == "custom" || "$VESPER_FFMPEG_PROFILE" == "remux-local" ]]; then
    VESPER_FFMPEG_CONFIGURE_ARGS+=(
      --disable-everything
      --disable-programs
      --disable-doc
      --disable-debug
      --disable-autodetect
      --disable-decoders
      --disable-encoders
      --disable-parsers
      --disable-bsfs
      --disable-protocols
      --disable-demuxers
      --disable-muxers
    )

    for library in avdevice avfilter swscale swresample; do
      if ! vesper_ffmpeg_array_contains "$library" ${VESPER_FFMPEG_FINAL_LIBRARIES[@]+"${VESPER_FFMPEG_FINAL_LIBRARIES[@]}"}; then
        VESPER_FFMPEG_CONFIGURE_ARGS+=("--disable-$library")
      fi
    done
  fi

  if [[ "$VESPER_FFMPEG_PROFILE" == "legacy" ]] || vesper_ffmpeg_protocols_need_network ${VESPER_FFMPEG_FINAL_PROTOCOLS[@]+"${VESPER_FFMPEG_FINAL_PROTOCOLS[@]}"}; then
    VESPER_FFMPEG_CONFIGURE_ARGS+=(--enable-network)
  else
    VESPER_FFMPEG_CONFIGURE_ARGS+=(--disable-network)
  fi

  case "$platform:$VESPER_FFMPEG_TLS_BACKEND" in
    android:openssl)
      VESPER_FFMPEG_CONFIGURE_ARGS+=(--enable-openssl --enable-version3)
      VESPER_FFMPEG_USE_OPENSSL=1
      VESPER_FFMPEG_EXTERNAL_DEPS+=(openssl)
      VESPER_FFMPEG_LICENSE_FLAGS+=(version3 openssl)
      ;;
    android:none)
      VESPER_FFMPEG_CONFIGURE_ARGS+=(--disable-openssl --disable-gnutls --disable-mbedtls --disable-securetransport)
      ;;
    apple:securetransport)
      VESPER_FFMPEG_CONFIGURE_ARGS+=(--enable-securetransport)
      ;;
    apple:none)
      VESPER_FFMPEG_CONFIGURE_ARGS+=(--disable-openssl --disable-gnutls --disable-mbedtls --disable-securetransport)
      ;;
  esac

  if [[ "$VESPER_FFMPEG_USE_LIBXML2" == "1" ]]; then
    VESPER_FFMPEG_CONFIGURE_ARGS+=(--enable-libxml2)
  fi

  for library in ${VESPER_FFMPEG_FINAL_LIBRARIES[@]+"${VESPER_FFMPEG_FINAL_LIBRARIES[@]}"}; do
    VESPER_FFMPEG_CONFIGURE_ARGS+=("--enable-$library")
  done
  for demuxer in ${VESPER_FFMPEG_FINAL_DEMUXERS[@]+"${VESPER_FFMPEG_FINAL_DEMUXERS[@]}"}; do
    VESPER_FFMPEG_CONFIGURE_ARGS+=("--enable-demuxer=$demuxer")
  done
  for muxer in ${VESPER_FFMPEG_FINAL_MUXERS[@]+"${VESPER_FFMPEG_FINAL_MUXERS[@]}"}; do
    VESPER_FFMPEG_CONFIGURE_ARGS+=("--enable-muxer=$muxer")
  done
  for protocol in ${VESPER_FFMPEG_FINAL_PROTOCOLS[@]+"${VESPER_FFMPEG_FINAL_PROTOCOLS[@]}"}; do
    VESPER_FFMPEG_CONFIGURE_ARGS+=("--enable-protocol=$protocol")
  done
  for parser in ${VESPER_FFMPEG_FINAL_PARSERS[@]+"${VESPER_FFMPEG_FINAL_PARSERS[@]}"}; do
    VESPER_FFMPEG_CONFIGURE_ARGS+=("--enable-parser=$parser")
  done
  for bsf in ${VESPER_FFMPEG_FINAL_BSFS[@]+"${VESPER_FFMPEG_FINAL_BSFS[@]}"}; do
    VESPER_FFMPEG_CONFIGURE_ARGS+=("--enable-bsf=$bsf")
  done

  VESPER_FFMPEG_CONFIGURE_ARGS+=(${VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS[@]+"${VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS[@]}"})

  if vesper_ffmpeg_has_flag --enable-gpl ${VESPER_FFMPEG_CONFIGURE_ARGS[@]+"${VESPER_FFMPEG_CONFIGURE_ARGS[@]}"} || vesper_ffmpeg_has_flag --enable-nonfree ${VESPER_FFMPEG_CONFIGURE_ARGS[@]+"${VESPER_FFMPEG_CONFIGURE_ARGS[@]}"}; then
    if [[ "$VESPER_FFMPEG_ACK_GPL_NONFREE" != "1" ]]; then
      echo "Refusing to build FFmpeg with GPL or nonfree flags without explicit acknowledgement." >&2
      echo "Pass --acknowledge-gpl-nonfree only when the release owner accepts the licensing consequences." >&2
      exit 1
    fi
    if vesper_ffmpeg_has_flag --enable-gpl ${VESPER_FFMPEG_CONFIGURE_ARGS[@]+"${VESPER_FFMPEG_CONFIGURE_ARGS[@]}"}; then
      VESPER_FFMPEG_LICENSE_FLAGS+=(gpl)
    fi
    if vesper_ffmpeg_has_flag --enable-nonfree ${VESPER_FFMPEG_CONFIGURE_ARGS[@]+"${VESPER_FFMPEG_CONFIGURE_ARGS[@]}"}; then
      VESPER_FFMPEG_LICENSE_FLAGS+=(nonfree)
    fi
  fi

  if [[ "$VESPER_FFMPEG_TLS_BACKEND" == "none" ]]; then
    for protocol in ${VESPER_FFMPEG_FINAL_PROTOCOLS[@]+"${VESPER_FFMPEG_FINAL_PROTOCOLS[@]}"}; do
      case "$protocol" in
        https|tls|rtmps|rtmpts)
          echo "Protocol $protocol requires a TLS backend, but --tls-backend none was selected." >&2
          exit 1
          ;;
      esac
    done
  fi
}

vesper_ffmpeg_configuration_seed() {
  local platform="$1"

  printf 'platform=%s\n' "$platform"
  printf 'profile=%s\n' "$VESPER_FFMPEG_PROFILE"
  printf 'tls_backend=%s\n' "$VESPER_FFMPEG_TLS_BACKEND"
  printf 'enable_dash=%s\n' "$VESPER_FFMPEG_ENABLE_DASH"
  printf 'libraries=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_LIBRARIES[@]+"${VESPER_FFMPEG_FINAL_LIBRARIES[@]}"})"
  printf 'demuxers=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_DEMUXERS[@]+"${VESPER_FFMPEG_FINAL_DEMUXERS[@]}"})"
  printf 'muxers=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_MUXERS[@]+"${VESPER_FFMPEG_FINAL_MUXERS[@]}"})"
  printf 'protocols=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_PROTOCOLS[@]+"${VESPER_FFMPEG_FINAL_PROTOCOLS[@]}"})"
  printf 'parsers=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_PARSERS[@]+"${VESPER_FFMPEG_FINAL_PARSERS[@]}"})"
  printf 'bsfs=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_BSFS[@]+"${VESPER_FFMPEG_FINAL_BSFS[@]}"})"
  printf 'extra_configure_args=%s\n' "$(vesper_ffmpeg_join_quoted ${VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS[@]+"${VESPER_FFMPEG_EXTRA_CONFIGURE_ARGS[@]}"})"
}

vesper_ffmpeg_profile_key() {
  local platform="$1"
  local seed

  if [[ "$VESPER_FFMPEG_PROFILE" == "legacy" && "$VESPER_FFMPEG_OVERLAY_COUNT" -eq 0 ]]; then
    echo "legacy"
    return 0
  fi

  seed="$(vesper_ffmpeg_configuration_seed "$platform")"
  printf '%s-%s\n' "$VESPER_FFMPEG_PROFILE" "$(vesper_ffmpeg_hash_text "$seed")"
}

vesper_ffmpeg_default_output_dir() {
  local platform="$1"
  local base_dir="$2"
  local profile_key

  profile_key="$(vesper_ffmpeg_profile_key "$platform")"
  if [[ "$profile_key" == "legacy" ]]; then
    echo "$base_dir"
  else
    echo "$base_dir/profiles/$profile_key"
  fi
}

vesper_ffmpeg_metadata_text() {
  local platform="$1"
  local target="$2"
  local ffmpeg_version="$3"
  local source_archive="$4"
  local source_url="$5"
  shift 5

  printf 'Vesper FFmpeg build metadata v1\n'
  printf 'platform=%s\n' "$platform"
  printf 'target=%s\n' "$target"
  printf 'profile=%s\n' "$VESPER_FFMPEG_PROFILE"
  printf 'declared_profile=%s\n' "${VESPER_DECLARED_FFMPEG_PROFILE:-$VESPER_FFMPEG_PROFILE}"
  printf 'declared_platform=%s\n' "${VESPER_DECLARED_FFMPEG_PLATFORM:-$platform}"
  printf 'profile_hash=%s\n' "$(vesper_ffmpeg_profile_key "$platform")"
  printf 'tls_backend=%s\n' "$VESPER_FFMPEG_TLS_BACKEND"
  printf 'enable_dash=%s\n' "$VESPER_FFMPEG_ENABLE_DASH"
  printf 'libraries=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_LIBRARIES[@]+"${VESPER_FFMPEG_FINAL_LIBRARIES[@]}"})"
  printf 'demuxers=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_DEMUXERS[@]+"${VESPER_FFMPEG_FINAL_DEMUXERS[@]}"})"
  printf 'muxers=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_MUXERS[@]+"${VESPER_FFMPEG_FINAL_MUXERS[@]}"})"
  printf 'protocols=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_PROTOCOLS[@]+"${VESPER_FFMPEG_FINAL_PROTOCOLS[@]}"})"
  printf 'parsers=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_PARSERS[@]+"${VESPER_FFMPEG_FINAL_PARSERS[@]}"})"
  printf 'bsfs=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_BSFS[@]+"${VESPER_FFMPEG_FINAL_BSFS[@]}"})"
  printf 'external_dependencies=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_EXTERNAL_DEPS[@]+"${VESPER_FFMPEG_EXTERNAL_DEPS[@]}"})"
  printf 'license_flags=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_LICENSE_FLAGS[@]+"${VESPER_FFMPEG_LICENSE_FLAGS[@]}"})"
  printf 'ffmpeg_version=%s\n' "$ffmpeg_version"
  printf 'source_archive=%s\n' "$source_archive"
  printf 'source_url=%s\n' "$source_url"
  printf 'configure_line='
  vesper_ffmpeg_join_quoted "$@"
  printf '\n'
}
