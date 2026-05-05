if [[ -n "${VESPER_COMMON_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_COMMON_SH_INCLUDED=1

VESPER_SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VESPER_REPO_ROOT="$(cd "$VESPER_SCRIPTS_DIR/.." && pwd)"

vesper_repo_root() {
  printf '%s\n' "$VESPER_REPO_ROOT"
}

vesper_scripts_dir() {
  printf '%s\n' "$VESPER_SCRIPTS_DIR"
}

vesper_require_command() {
  local command_name="$1"
  local message="${2:-Missing required command: $command_name}"

  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "$message" >&2
    exit 1
  fi
}

vesper_source_cargo_env_for_xcode() {
  if [[ -f "${HOME:-}/.cargo/env" ]]; then
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
  fi

  export PATH="${HOME:-}/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
}

vesper_require_rust_tools_for_xcode() {
  local tool

  vesper_source_cargo_env_for_xcode
  for tool in rustup cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "$tool was not found in PATH. Install Rust or expose $tool to Xcode script phases." >&2
      echo "Current PATH: $PATH" >&2
      exit 1
    fi
  done
}

vesper_download_if_missing() {
  local archive_path="$1"
  shift
  local archive_url
  local download_succeeded=0
  local curl_output
  local -a curl_failures=()

  if [[ -f "$archive_path" ]]; then
    return 0
  fi

  vesper_require_command curl "curl is required to download source archives."
  mkdir -p "$(dirname "$archive_path")"

  for archive_url in "$@"; do
    echo "Downloading source archive:"
    echo "  $archive_url"
    if curl_output="$(curl --fail --location --silent --show-error --output "$archive_path" "$archive_url" 2>&1)"; then
      download_succeeded=1
      break
    fi

    rm -f "$archive_path"
    if [[ -n "$curl_output" ]]; then
      curl_failures+=("$archive_url"$'\n'"$curl_output")
    fi
    echo "Source download failed for $archive_url, trying next mirror if available." >&2
  done

  if [[ "$download_succeeded" != "1" ]]; then
    echo "Unable to download source archive into:" >&2
    echo "  $archive_path" >&2
    echo "Tried source URLs:" >&2
    for archive_url in "$@"; do
      echo "  $archive_url" >&2
    done
    if [[ ${#curl_failures[@]} -gt 0 ]]; then
      echo "curl failure details:" >&2
      for curl_output in "${curl_failures[@]}"; do
        printf '%s\n' "$curl_output" >&2
      done
    fi
    exit 1
  fi
}

vesper_extract_source_tree() {
  local archive_path="$1"
  local destination_dir="$2"

  rm -rf "$destination_dir"
  mkdir -p "$destination_dir"
  tar -xf "$archive_path" -C "$destination_dir" --strip-components=1
}

vesper_make_jobs() {
  if command -v getconf >/dev/null 2>&1; then
    getconf _NPROCESSORS_ONLN
    return 0
  fi

  if command -v sysctl >/dev/null 2>&1; then
    sysctl -n hw.ncpu
    return 0
  fi

  echo 4
}

vesper_path_cache_key() {
  local path="$1"
  local sanitized="${path#/}"

  sanitized="${sanitized//\//_}"
  sanitized="${sanitized//:/_}"
  sanitized="${sanitized// /_}"

  printf '%s\n' "$sanitized"
}
