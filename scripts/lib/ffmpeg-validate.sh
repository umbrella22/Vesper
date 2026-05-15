if [[ -n "${VESPER_FFMPEG_VALIDATE_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_FFMPEG_VALIDATE_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

vesper_ffmpeg_validation_csv_contains() {
  local csv="$1"
  local needle="$2"
  local token

  csv="${csv//,/ }"
  for token in $csv; do
    [[ "$token" == "$needle" ]] && return 0
  done
  return 1
}

vesper_ffmpeg_validation_protocol_is_network() {
  case "$1" in
    async|cache|concatf|crypto|data|ffrtmpcrypt|ftp|gopher|gophers|hls|http|httpproxy|https|icecast|mmsh|mmst|rtmp|rtmpe|rtmps|rtmpt|rtmpte|rtmpts|rtp|sctp|srtp|subfile|tcp|tls|udp|unix)
      return 0
      ;;
  esac
  return 1
}

vesper_ffmpeg_validate_resolved_profile() {
  local protocols_csv="$1"
  local tls_backend="$2"
  local forbid_network="${3:-false}"
  local forbid_openssl="${4:-false}"
  shift 4 || true
  local protocol extra_arg extra_protocol

  if [[ "$forbid_network" == "true" ]]; then
    for protocol in ${protocols_csv//,/ }; do
      if vesper_ffmpeg_validation_protocol_is_network "$protocol"; then
        echo "FFmpeg profile forbids network but enables protocol: $protocol" >&2
        exit 1
      fi
    done
    for extra_arg in "$@"; do
      case "$extra_arg" in
        --enable-network)
          echo "FFmpeg profile forbids network but enables configure flag: $extra_arg" >&2
          exit 1
          ;;
        --enable-protocol=*)
          extra_protocol="${extra_arg#*=}"
          if vesper_ffmpeg_validation_protocol_is_network "$extra_protocol"; then
            echo "FFmpeg profile forbids network but enables protocol configure flag: $extra_arg" >&2
            exit 1
          fi
          ;;
      esac
    done
  fi

  if [[ "$forbid_openssl" == "true" && "$tls_backend" == "openssl" ]]; then
    echo "FFmpeg profile forbids OpenSSL but selects tls=openssl." >&2
    exit 1
  fi
  if [[ "$forbid_openssl" == "true" ]]; then
    for extra_arg in "$@"; do
      case "$extra_arg" in
        --enable-openssl|--enable-openssl=*)
          echo "FFmpeg profile forbids OpenSSL but enables configure flag: $extra_arg" >&2
          exit 1
          ;;
      esac
    done
  fi
}

vesper_ffmpeg_validate_metadata_file() {
  local metadata_file="$1"
  local forbid_network="${2:-false}"
  local forbid_openssl="${3:-false}"

  if [[ "$forbid_network" == "true" ]]; then
    if ! grep -q -- '--disable-network' "$metadata_file"; then
      echo "FFmpeg metadata does not include --disable-network: $metadata_file" >&2
      exit 1
    fi
    if grep -Eq -- '--enable-network|protocols=.*(http|https|tcp|tls|rtmp|rtmps|rtmpt|rtmpts)' "$metadata_file"; then
      echo "FFmpeg metadata includes forbidden network capability: $metadata_file" >&2
      exit 1
    fi
  fi

  if [[ "$forbid_openssl" == "true" ]]; then
    if ! grep -q -- '--disable-openssl' "$metadata_file"; then
      echo "FFmpeg metadata does not include --disable-openssl: $metadata_file" >&2
      exit 1
    fi
    if grep -Eq -- '--enable-openssl|external_dependencies=.*openssl|license_flags=.*openssl' "$metadata_file"; then
      echo "FFmpeg metadata includes forbidden OpenSSL capability: $metadata_file" >&2
      exit 1
    fi
  fi
}

vesper_ffmpeg_validate_metadata_tree() {
  local root="$1"
  local forbid_network="${2:-false}"
  local forbid_openssl="${3:-false}"
  local metadata_file

  [[ -d "$root" ]] || return 0
  while IFS= read -r metadata_file; do
    vesper_ffmpeg_validate_metadata_file "$metadata_file" "$forbid_network" "$forbid_openssl"
  done < <(find "$root" -type f \( -name '*metadata.txt' -o -name 'vesper-ffmpeg-build-metadata.txt' \) 2>/dev/null | sort)
}

vesper_ffmpeg_validate_android_runtime_artifacts() {
  local runtime_module_dir="$1"
  local forbid_network="${2:-false}"
  local forbid_openssl="${3:-false}"
  local unexpected_crypto aar_path unpack_dir tmp_dir

  vesper_ffmpeg_validate_metadata_tree "$runtime_module_dir/src/main/assets" "$forbid_network" "$forbid_openssl"

  if [[ "$forbid_openssl" == "true" ]]; then
    unexpected_crypto="$(
      find "$runtime_module_dir/src/main" "$runtime_module_dir/build/outputs/aar" -type f \
        \( -name 'libssl*.so' -o -name 'libcrypto*.so' \) \
        -print -quit 2>/dev/null || true
    )"
    if [[ -n "$unexpected_crypto" ]]; then
      echo "FFmpeg runtime packaged forbidden OpenSSL payload:" >&2
      echo "  $unexpected_crypto" >&2
      exit 1
    fi
  fi

  tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/vesper-ffmpeg-runtime-verify.XXXXXX")"

  while IFS= read -r aar_path; do
    unpack_dir="$tmp_dir/$(basename "$aar_path" .aar)"
    mkdir -p "$unpack_dir"
    unzip -q "$aar_path" -d "$unpack_dir"
    if [[ "$forbid_openssl" == "true" ]]; then
      unexpected_crypto="$(
        find "$unpack_dir" -type f \( -name 'libssl*.so' -o -name 'libcrypto*.so' \) -print -quit
      )"
      if [[ -n "$unexpected_crypto" ]]; then
        echo "FFmpeg runtime AAR contains forbidden OpenSSL payload:" >&2
        echo "  $unexpected_crypto" >&2
        exit 1
      fi
    fi
  done < <(find "$runtime_module_dir/build/outputs/aar" -type f -name '*.aar' 2>/dev/null | sort)
  rm -rf "$tmp_dir"
}
