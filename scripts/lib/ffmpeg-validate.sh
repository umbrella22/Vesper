if [[ -n "${VESPER_FFMPEG_VALIDATE_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_FFMPEG_VALIDATE_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ffmpeg.sh"

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
    if ! LC_ALL=C grep -q -- '--disable-network' "$metadata_file"; then
      echo "FFmpeg metadata does not include --disable-network: $metadata_file" >&2
      exit 1
    fi
    if LC_ALL=C grep -Eq -- '--enable-network|protocols=.*(http|https|tcp|tls|rtmp|rtmps|rtmpt|rtmpts)' "$metadata_file"; then
      echo "FFmpeg metadata includes forbidden network capability: $metadata_file" >&2
      exit 1
    fi
  fi

  if [[ "$forbid_openssl" == "true" ]]; then
    if ! LC_ALL=C grep -q -- '--disable-openssl' "$metadata_file"; then
      echo "FFmpeg metadata does not include --disable-openssl: $metadata_file" >&2
      exit 1
    fi
    if LC_ALL=C grep -Eq -- '--enable-openssl|external_dependencies=.*openssl|license_flags=.*openssl' "$metadata_file"; then
      echo "FFmpeg metadata includes forbidden OpenSSL capability: $metadata_file" >&2
      exit 1
    fi
  fi
}

vesper_ffmpeg_metadata_value() {
  local metadata_file="$1"
  local key="$2"
  local value
  local status

  if value="$(awk -F= -v expected="$key" '
    $1 == expected {
      value = substr($0, index($0, "=") + 1)
      count += 1
    }
    END {
      if (count == 0) exit 1
      if (count > 1) exit 2
      print value
    }
  ' "$metadata_file")"; then
    printf '%s\n' "$value"
    return 0
  else
    status=$?
  fi
  if [[ "$status" -eq 2 ]]; then
    echo "Duplicate FFmpeg metadata key '$key': $metadata_file" >&2
  else
    echo "Missing FFmpeg metadata key '$key': $metadata_file" >&2
  fi
  return 1
}

vesper_ffmpeg_validate_lgpl_shared_metadata_file() {
  local metadata_file="$1"
  local license_flags
  local configure_line

  if [[ ! -f "$metadata_file" ]]; then
    echo "Missing FFmpeg build metadata: $metadata_file" >&2
    return 1
  fi

  license_flags="$(vesper_ffmpeg_metadata_value "$metadata_file" license_flags)" || return 1
  if [[ -n "$license_flags" ]]; then
    echo "The default release requires LGPL-oriented FFmpeg metadata; license_flags is not empty:" >&2
    echo "  $metadata_file: $license_flags" >&2
    return 1
  fi

  configure_line="$(vesper_ffmpeg_metadata_value "$metadata_file" configure_line)" || return 1
  if [[ "$configure_line" == *"--enable-gpl"* || "$configure_line" == *"--enable-nonfree"* ]]; then
    echo "GPL or nonfree FFmpeg configure flags are not allowed in the default release:" >&2
    echo "  $metadata_file" >&2
    return 1
  fi
  if [[ " $configure_line " != *" --enable-shared "* ]]; then
    echo "The default release requires shared FFmpeg libraries: $metadata_file" >&2
    return 1
  fi
}

vesper_ffmpeg_verified_binary_sha256() {
  local binary_path="$1"
  local checksum_path="$2"
  local recorded_sha256
  local actual_sha256

  if [[ ! -f "$binary_path" || ! -f "$checksum_path" ]]; then
    echo "Missing FFmpeg-backed framework binary checksum input:" >&2
    echo "  binary:   $binary_path" >&2
    echo "  checksum: $checksum_path" >&2
    return 1
  fi
  recorded_sha256="$(tr -d '[:space:]' <"$checksum_path")"
  if [[ ! "$recorded_sha256" =~ ^[0-9a-f]{64}$ ]]; then
    echo "Invalid FFmpeg-backed framework binary SHA-256 record: $checksum_path" >&2
    return 1
  fi
  actual_sha256="$(vesper_ffmpeg_sha256_file "$binary_path")" || return 1
  if [[ "$actual_sha256" != "$recorded_sha256" ]]; then
    echo "FFmpeg-backed framework binary SHA-256 mismatch:" >&2
    echo "  binary:   $binary_path" >&2
    echo "  recorded: $recorded_sha256" >&2
    echo "  actual:   $actual_sha256" >&2
    return 1
  fi
  printf '%s\n' "$actual_sha256"
}

vesper_ffmpeg_build_input_fingerprint() {
  local ffmpeg_dir="$1"
  local metadata_path="$ffmpeg_dir/vesper-ffmpeg-build-metadata.txt"
  local library_checksums_path="$ffmpeg_dir/vesper-ffmpeg-library-sha256.txt"

  if [[ ! -f "$metadata_path" || ! -f "$library_checksums_path" ]]; then
    echo "Missing FFmpeg build input provenance under: $ffmpeg_dir" >&2
    return 1
  fi
  printf '%s-%s\n' \
    "$(vesper_ffmpeg_sha256_file "$metadata_path")" \
    "$(vesper_ffmpeg_sha256_file "$library_checksums_path")"
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
