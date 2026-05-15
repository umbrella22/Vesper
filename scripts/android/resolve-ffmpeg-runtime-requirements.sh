#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONSUMER_DIR="$ROOT_DIR/scripts/android/ffmpeg-consumers"
PRINT_METADATA=0

if [[ "${1:-}" == "--print-metadata" ]]; then
  PRINT_METADATA=1
  shift
fi

seen_keys=()
libraries=()
demuxers=()
muxers=()
protocols=()
parsers=()
bsfs=()

seen_contains() {
  local needle="$1"
  local value
  for value in ${seen_keys[@]+"${seen_keys[@]}"}; do
    [[ "$value" == "$needle" ]] && return 0
  done
  return 1
}

append_unique() {
  local target="$1"
  local value="$2"
  local key="$target:$value"
  [[ -n "$value" ]] || return 0
  if seen_contains "$key"; then
    return 0
  fi
  seen_keys+=("$key")
  eval "$target+=(\"\$value\")"
}

append_csv() {
  local target="$1"
  local csv="$2"
  local token
  csv="${csv//,/ }"
  for token in $csv; do
    append_unique "$target" "$token"
  done
}

load_manifest() {
  local name="$1"
  local path="$CONSUMER_DIR/$name.requirements"
  local key value

  if [[ ! -f "$path" ]]; then
    echo "Unknown Android FFmpeg runtime consumer: $name" >&2
    echo "Known consumers:" >&2
    find "$CONSUMER_DIR" -maxdepth 1 -name '*.requirements' -type f -exec basename {} .requirements \; | sort >&2
    exit 1
  fi

  while IFS='=' read -r key value; do
    [[ -n "${key:-}" ]] || continue
    [[ "$key" != \#* ]] || continue
    case "$key" in
      libraries) append_csv libraries "${value:-}" ;;
      demuxers) append_csv demuxers "${value:-}" ;;
      muxers) append_csv muxers "${value:-}" ;;
      protocols) append_csv protocols "${value:-}" ;;
      parsers) append_csv parsers "${value:-}" ;;
      bsfs) append_csv bsfs "${value:-}" ;;
      *)
        echo "Unknown requirement key '$key' in $path" >&2
        exit 1
        ;;
    esac
  done <"$path"
}

join_csv() {
  local separator=""
  local value
  for value in "$@"; do
    printf '%s%s' "$separator" "$value"
    separator=","
  done
}

array_contains() {
  local needle="$1"
  local value
  shift
  for value in "$@"; do
    [[ "$value" == "$needle" ]] && return 0
  done
  return 1
}

consumers=("$@")
if [[ ${#consumers[@]} -eq 0 ]]; then
  consumers=(download-remux relay-remux)
fi

load_manifest base
for consumer in "${consumers[@]}"; do
  [[ "$consumer" != "base" ]] || continue
  load_manifest "$consumer"
done

tls_backend="none"
for protocol in "${protocols[@]}"; do
  case "$protocol" in
    https|tls|rtmps|rtmpts)
      tls_backend="openssl"
      ;;
  esac
done

enable_dash=0
if array_contains dash "${demuxers[@]}"; then
  enable_dash=1
fi

resolved_args=(
  "--ffmpeg-profile" "custom"
  "--tls-backend" "$tls_backend"
  "--enable-libraries" "$(join_csv "${libraries[@]}")"
  "--enable-demuxers" "$(join_csv "${demuxers[@]}")"
  "--enable-muxers" "$(join_csv "${muxers[@]}")"
  "--enable-protocols" "$(join_csv "${protocols[@]}")"
  "--enable-parsers" "$(join_csv "${parsers[@]}")"
  "--enable-bsfs" "$(join_csv "${bsfs[@]}")"
)
if [[ "$enable_dash" == "1" ]]; then
  resolved_args+=("--enable-dash")
else
  resolved_args+=("--disable-dash")
fi

if [[ "$PRINT_METADATA" == "1" ]]; then
  # shellcheck source=../lib/ffmpeg.sh
  source "$ROOT_DIR/scripts/lib/ffmpeg.sh"
  vesper_ffmpeg_parse_common_args android "${resolved_args[@]}"
  printf 'consumers=%s\n' "${consumers[*]}"
  printf 'profile_hash=%s\n' "$(vesper_ffmpeg_profile_key android)"
  printf 'libraries=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_LIBRARIES[@]+"${VESPER_FFMPEG_FINAL_LIBRARIES[@]}"})"
  printf 'demuxers=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_DEMUXERS[@]+"${VESPER_FFMPEG_FINAL_DEMUXERS[@]}"})"
  printf 'muxers=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_MUXERS[@]+"${VESPER_FFMPEG_FINAL_MUXERS[@]}"})"
  printf 'protocols=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_PROTOCOLS[@]+"${VESPER_FFMPEG_FINAL_PROTOCOLS[@]}"})"
  printf 'parsers=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_PARSERS[@]+"${VESPER_FFMPEG_FINAL_PARSERS[@]}"})"
  printf 'bitstream_filters=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_FINAL_BSFS[@]+"${VESPER_FFMPEG_FINAL_BSFS[@]}"})"
  printf 'external_dependencies=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_EXTERNAL_DEPS[@]+"${VESPER_FFMPEG_EXTERNAL_DEPS[@]}"})"
  printf 'license_flags=%s\n' "$(vesper_ffmpeg_join_csv ${VESPER_FFMPEG_LICENSE_FLAGS[@]+"${VESPER_FFMPEG_LICENSE_FLAGS[@]}"})"
  exit 0
fi

printf '%s\n' "${resolved_args[@]}"
