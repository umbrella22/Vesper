if [[ -n "${VESPER_FFMPEG_PROFILE_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_FFMPEG_PROFILE_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

VESPER_FFMPEG_PROFILE_CONFIG_PATH="${VESPER_FFMPEG_PROFILE_CONFIG_PATH:-$VESPER_REPO_ROOT/scripts/ffmpeg-profiles.toml}"
VESPER_FFMPEG_PROFILE_SECTIONS=""

vesper_ffmpeg_profile_trim() {
  printf '%s' "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'
}

vesper_ffmpeg_profile_sanitize() {
  printf '%s' "$1" | tr '[:lower:]-.' '[:upper:]__' | tr -c 'A-Z0-9_' '_'
}

vesper_ffmpeg_profile_var_name() {
  local section="$1"
  local key="$2"
  printf 'VESPER_FFMPEG_PROFILE_TOML__%s__%s' \
    "$(vesper_ffmpeg_profile_sanitize "$section")" \
    "$(vesper_ffmpeg_profile_sanitize "$key")"
}

vesper_ffmpeg_profile_section_seen() {
  local section="$1"
  [[ " $VESPER_FFMPEG_PROFILE_SECTIONS " == *" $section "* ]]
}

vesper_ffmpeg_profile_note_section() {
  local section="$1"
  if ! vesper_ffmpeg_profile_section_seen "$section"; then
    VESPER_FFMPEG_PROFILE_SECTIONS="$VESPER_FFMPEG_PROFILE_SECTIONS $section"
  fi
}

vesper_ffmpeg_profile_parse_value() {
  local value="$1"
  value="$(vesper_ffmpeg_profile_trim "$value")"
  case "$value" in
    \[*\])
      value="${value#\[}"
      value="${value%\]}"
      value="${value//\"/}"
      value="$(printf '%s' "$value" | tr -d '[:space:]')"
      ;;
    \"*\")
      value="${value#\"}"
      value="${value%\"}"
      ;;
  esac
  printf '%s' "$value"
}

vesper_ffmpeg_profile_set() {
  local section="$1"
  local key="$2"
  local value="$3"
  local var_name
  var_name="$(vesper_ffmpeg_profile_var_name "$section" "$key")"
  printf -v "$var_name" '%s' "$value"
}

vesper_ffmpeg_profile_get() {
  local section="$1"
  local key="$2"
  local var_name
  var_name="$(vesper_ffmpeg_profile_var_name "$section" "$key")"
  printf '%s' "${!var_name:-}"
}

vesper_ffmpeg_profile_load() {
  local path="${1:-$VESPER_FFMPEG_PROFILE_CONFIG_PATH}"
  local raw line section key value

  if [[ "${VESPER_FFMPEG_PROFILE_LOADED_PATH:-}" == "$path" ]]; then
    return 0
  fi
  if [[ ! -f "$path" ]]; then
    echo "FFmpeg profile config not found: $path" >&2
    exit 1
  fi

  VESPER_FFMPEG_PROFILE_SECTIONS=""
  section=""
  while IFS= read -r raw || [[ -n "$raw" ]]; do
    line="${raw%%#*}"
    line="$(vesper_ffmpeg_profile_trim "$line")"
    [[ -n "$line" ]] || continue

    if [[ "$line" == \[*\] ]]; then
      section="${line#\[}"
      section="${section%\]}"
      section="$(vesper_ffmpeg_profile_trim "$section")"
      vesper_ffmpeg_profile_note_section "$section"
      continue
    fi

    if [[ "$line" != *=* || -z "$section" ]]; then
      echo "Unsupported TOML line in $path: $raw" >&2
      exit 1
    fi

    key="$(vesper_ffmpeg_profile_trim "${line%%=*}")"
    value="$(vesper_ffmpeg_profile_parse_value "${line#*=}")"
    vesper_ffmpeg_profile_set "$section" "$key" "$value"
  done <"$path"

  VESPER_FFMPEG_PROFILE_LOADED_PATH="$path"
}

vesper_ffmpeg_profile_append_unique() {
  local target="$1"
  local value="$2"
  local existing
  local existing_values=()
  local restore_nounset=0

  [[ -n "$value" ]] || return 0
  if [[ "$-" == *u* ]]; then
    restore_nounset=1
    set +u
  fi
  eval "existing_values=(\"\${${target}[@]}\")"
  for existing in "${existing_values[@]}"; do
    if [[ "$existing" == "$value" ]]; then
      if [[ "$restore_nounset" == "1" ]]; then
        set -u
      fi
      return 0
    fi
  done
  eval "$target+=(\"\$value\")"
  if [[ "$restore_nounset" == "1" ]]; then
    set -u
  fi
}

vesper_ffmpeg_profile_append_csv() {
  local target="$1"
  local csv="$2"
  local token

  csv="${csv//,/ }"
  for token in $csv; do
    vesper_ffmpeg_profile_append_unique "$target" "$token"
  done
}

vesper_ffmpeg_profile_join_csv() {
  local separator=""
  local value

  for value in "$@"; do
    printf '%s%s' "$separator" "$value"
    separator=","
  done
}

vesper_ffmpeg_profile_contains_token() {
  local csv="$1"
  local needle="$2"
  local token

  csv="${csv//,/ }"
  for token in $csv; do
    [[ "$token" == "$needle" ]] && return 0
  done
  return 1
}

vesper_ffmpeg_profile_reset_resolved() {
  VESPER_PROFILE_RESOLVED_LIBRARIES=()
  VESPER_PROFILE_RESOLVED_DEMUXERS=()
  VESPER_PROFILE_RESOLVED_MUXERS=()
  VESPER_PROFILE_RESOLVED_PROTOCOLS=()
  VESPER_PROFILE_RESOLVED_PARSERS=()
  VESPER_PROFILE_RESOLVED_BSFS=()
  VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS=()
  VESPER_PROFILE_RESOLVED_TLS_BACKEND=""
  VESPER_PROFILE_VALIDATION_FORBID_NETWORK=""
  VESPER_PROFILE_VALIDATION_FORBID_OPENSSL=""
  VESPER_PROFILE_RESOLVE_STACK=""
}

vesper_ffmpeg_profile_apply_table() {
  local section="$1"
  local value

  value="$(vesper_ffmpeg_profile_get "$section" libraries)"
  [[ -z "$value" ]] || vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_LIBRARIES "$value"
  value="$(vesper_ffmpeg_profile_get "$section" demuxers)"
  [[ -z "$value" ]] || vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_DEMUXERS "$value"
  value="$(vesper_ffmpeg_profile_get "$section" muxers)"
  [[ -z "$value" ]] || vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_MUXERS "$value"
  value="$(vesper_ffmpeg_profile_get "$section" protocols)"
  [[ -z "$value" ]] || vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_PROTOCOLS "$value"
  value="$(vesper_ffmpeg_profile_get "$section" parsers)"
  [[ -z "$value" ]] || vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_PARSERS "$value"
  value="$(vesper_ffmpeg_profile_get "$section" bsfs)"
  [[ -z "$value" ]] || vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_BSFS "$value"
  value="$(vesper_ffmpeg_profile_get "$section" extra_configure_args)"
  [[ -z "$value" ]] || vesper_ffmpeg_profile_append_csv VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS "$value"
  value="$(vesper_ffmpeg_profile_get "$section" tls)"
  [[ -z "$value" ]] || VESPER_PROFILE_RESOLVED_TLS_BACKEND="$value"
}

vesper_ffmpeg_profile_apply_validation() {
  local section="$1"
  local value

  value="$(vesper_ffmpeg_profile_get "$section" forbid_network)"
  [[ -z "$value" ]] || VESPER_PROFILE_VALIDATION_FORBID_NETWORK="$value"
  value="$(vesper_ffmpeg_profile_get "$section" forbid_openssl)"
  [[ -z "$value" ]] || VESPER_PROFILE_VALIDATION_FORBID_OPENSSL="$value"
}

vesper_ffmpeg_profile_resolve_one() {
  local profile="$1"
  local platform="$2"
  local section="profile.$profile"
  local extends parent

  if ! vesper_ffmpeg_profile_section_seen "$section"; then
    echo "Unknown FFmpeg profile: $profile" >&2
    echo "Known profiles:" >&2
    vesper_ffmpeg_profile_list_names | sed 's/^/  /' >&2
    exit 1
  fi

  if [[ " $VESPER_PROFILE_RESOLVE_STACK " == *" $profile "* ]]; then
    echo "FFmpeg profile inheritance cycle at: $profile" >&2
    exit 1
  fi
  VESPER_PROFILE_RESOLVE_STACK="$VESPER_PROFILE_RESOLVE_STACK $profile"

  extends="$(vesper_ffmpeg_profile_get "$section" extends)"
  extends="${extends//,/ }"
  for parent in $extends; do
    [[ -n "$parent" ]] || continue
    vesper_ffmpeg_profile_resolve_one "$parent" "$platform"
  done

  vesper_ffmpeg_profile_apply_table "$section"
  vesper_ffmpeg_profile_apply_validation "$section.validation"
  vesper_ffmpeg_profile_apply_table "$section.platform_overrides.$platform"
  vesper_ffmpeg_profile_apply_validation "$section.platform_overrides.$platform.validation"
  VESPER_PROFILE_RESOLVE_STACK="${VESPER_PROFILE_RESOLVE_STACK% $profile}"
}

vesper_ffmpeg_profile_resolve() {
  local profile="$1"
  local platform="$2"
  local path="${3:-$VESPER_FFMPEG_PROFILE_CONFIG_PATH}"

  vesper_ffmpeg_profile_load "$path"
  vesper_ffmpeg_profile_reset_resolved
  vesper_ffmpeg_profile_resolve_one "$profile" "$platform"
  if [[ -z "$VESPER_PROFILE_RESOLVED_TLS_BACKEND" ]]; then
    VESPER_PROFILE_RESOLVED_TLS_BACKEND="none"
  fi
}

vesper_ffmpeg_profile_list_names() {
  local section name

  vesper_ffmpeg_profile_load "$VESPER_FFMPEG_PROFILE_CONFIG_PATH"
  for section in $VESPER_FFMPEG_PROFILE_SECTIONS; do
    case "$section" in
      profile.*.validation|profile.*.platform_overrides.*|profile.*.platform_overrides.*.validation)
        ;;
      profile.*)
        name="${section#profile.}"
        printf '%s\n' "$name"
        ;;
    esac
  done
}

vesper_ffmpeg_profile_export_validation_env() {
  export VESPER_FFMPEG_VALIDATION_FORBID_NETWORK="$VESPER_PROFILE_VALIDATION_FORBID_NETWORK"
  export VESPER_FFMPEG_VALIDATION_FORBID_OPENSSL="$VESPER_PROFILE_VALIDATION_FORBID_OPENSSL"
}

vesper_ffmpeg_profile_emit_legacy_args() {
  local dash=0
  local csv
  local restore_nounset=0

  printf '%s\n' --ffmpeg-profile custom
  printf '%s\n' --tls-backend "$VESPER_PROFILE_RESOLVED_TLS_BACKEND"

  csv="$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_LIBRARIES[@]+"${VESPER_PROFILE_RESOLVED_LIBRARIES[@]}"})"
  [[ -z "$csv" ]] || printf '%s\n' --enable-libraries "$csv"
  csv="$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_DEMUXERS[@]+"${VESPER_PROFILE_RESOLVED_DEMUXERS[@]}"})"
  [[ -z "$csv" ]] || printf '%s\n' --enable-demuxers "$csv"
  if vesper_ffmpeg_profile_contains_token "$csv" dash; then
    dash=1
  fi
  csv="$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_MUXERS[@]+"${VESPER_PROFILE_RESOLVED_MUXERS[@]}"})"
  [[ -z "$csv" ]] || printf '%s\n' --enable-muxers "$csv"
  csv="$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]+"${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]}"})"
  [[ -z "$csv" ]] || printf '%s\n' --enable-protocols "$csv"
  csv="$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_PARSERS[@]+"${VESPER_PROFILE_RESOLVED_PARSERS[@]}"})"
  [[ -z "$csv" ]] || printf '%s\n' --enable-parsers "$csv"
  csv="$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_BSFS[@]+"${VESPER_PROFILE_RESOLVED_BSFS[@]}"})"
  [[ -z "$csv" ]] || printf '%s\n' --enable-bsfs "$csv"
  if [[ "$-" == *u* ]]; then
    restore_nounset=1
    set +u
  fi
  for csv in "${VESPER_PROFILE_RESOLVED_EXTRA_CONFIGURE_ARGS[@]}"; do
    printf '%s\n' --extra-configure-arg "$csv"
  done
  if [[ "$restore_nounset" == "1" ]]; then
    set -u
  fi

  if [[ "$dash" == "1" ]]; then
    printf '%s\n' --enable-dash
  else
    printf '%s\n' --disable-dash
  fi
}

vesper_ffmpeg_profile_print_resolved() {
  local profile="$1"
  local platform="$2"

  printf 'profile=%s\n' "$profile"
  printf 'platform=%s\n' "$platform"
  printf 'libraries=%s\n' "$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_LIBRARIES[@]+"${VESPER_PROFILE_RESOLVED_LIBRARIES[@]}"})"
  printf 'demuxers=%s\n' "$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_DEMUXERS[@]+"${VESPER_PROFILE_RESOLVED_DEMUXERS[@]}"})"
  printf 'muxers=%s\n' "$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_MUXERS[@]+"${VESPER_PROFILE_RESOLVED_MUXERS[@]}"})"
  printf 'protocols=%s\n' "$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]+"${VESPER_PROFILE_RESOLVED_PROTOCOLS[@]}"})"
  printf 'parsers=%s\n' "$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_PARSERS[@]+"${VESPER_PROFILE_RESOLVED_PARSERS[@]}"})"
  printf 'bsfs=%s\n' "$(vesper_ffmpeg_profile_join_csv ${VESPER_PROFILE_RESOLVED_BSFS[@]+"${VESPER_PROFILE_RESOLVED_BSFS[@]}"})"
  printf 'tls=%s\n' "$VESPER_PROFILE_RESOLVED_TLS_BACKEND"
  printf 'validation.forbid_network=%s\n' "${VESPER_PROFILE_VALIDATION_FORBID_NETWORK:-false}"
  printf 'validation.forbid_openssl=%s\n' "${VESPER_PROFILE_VALIDATION_FORBID_OPENSSL:-false}"
}
