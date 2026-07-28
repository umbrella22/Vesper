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
  for tool in rustc cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "$tool was not found in PATH. Install Rust or expose $tool to Xcode script phases." >&2
      echo "Current PATH: $PATH" >&2
      exit 1
    fi
  done
}

vesper_rustup_is_toolchain_manager() {
  command -v rustup >/dev/null 2>&1 &&
    rustup --version 2>/dev/null | head -n 1 | grep -Eq '^rustup [0-9]'
}

vesper_rust_target_is_installed() {
  local target="$1"
  local target_libdir

  if ! command -v rustc >/dev/null 2>&1; then
    return 1
  fi

  if ! target_libdir="$(rustc --print target-libdir --target "$target" 2>/dev/null)"; then
    return 1
  fi

  [[ -d "$target_libdir" ]]
}

vesper_require_rust_targets() {
  local label="$1"
  shift
  local target
  local -a missing_targets=()

  for target in "$@"; do
    if ! vesper_rust_target_is_installed "$target"; then
      missing_targets+=("$target")
    fi
  done

  if [[ ${#missing_targets[@]} -eq 0 ]]; then
    return 0
  fi

  echo "Required Rust $label targets are missing:" >&2
  for target in "${missing_targets[@]}"; do
    echo "  $target" >&2
  done
  echo >&2

  if vesper_rustup_is_toolchain_manager; then
    echo "Install them with:" >&2
    echo "  rustup target add ${missing_targets[*]}" >&2
  else
    echo "A usable rustup toolchain manager was not found in PATH." >&2
    if command -v rustup >/dev/null 2>&1; then
      echo "Current rustup path: $(command -v rustup)" >&2
      echo "Current rustup version output:" >&2
      rustup --version 2>&1 | sed 's/^/  /' >&2 || true
    else
      echo "Current PATH has no rustup command." >&2
    fi
    echo "Install Rust targets before running this script." >&2
  fi
  exit 1
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

vesper_test_fixture_path_pattern() {
  printf '%s\n' '(^|/)(subtitle_contract|test[-_]?fixtures?|test[-_]?assets?|testdata)(/|$)|(^|/)fixtures/(contracts|media)(/|$)|(^|/)(tiny-aac\.m4a|tiny-h264-aac(-mediacodec)?\.m4v)$'
}

vesper_test_fixture_binary_marker_pattern() {
  printf '%s\n' 'assets/subtitle_contract|fixtures/(contracts|media)|tiny-aac\.m4a|tiny-h264-aac(-mediacodec)?\.m4v'
}

vesper_verify_archive_excludes_test_fixtures() {
  local archive_path="$1"
  local entries
  local matches
  local pattern

  if [[ ! -f "$archive_path" ]]; then
    echo "Release archive does not exist: $archive_path" >&2
    return 1
  fi

  vesper_require_command unzip
  if ! entries="$(unzip -Z1 "$archive_path")"; then
    echo "Unable to list release archive entries: $archive_path" >&2
    return 1
  fi

  pattern="$(vesper_test_fixture_path_pattern)"
  matches="$(printf '%s\n' "$entries" | grep -Ei "$pattern" || true)"
  if [[ -n "$matches" ]]; then
    echo "Release archive contains test fixture resources: $archive_path" >&2
    printf '  %s\n' "$matches" >&2
    return 1
  fi
}

vesper_verify_directory_excludes_test_fixtures() {
  local directory_path="$1"
  local matches
  local pattern

  if [[ ! -d "$directory_path" ]]; then
    echo "Release directory does not exist: $directory_path" >&2
    return 1
  fi

  pattern="$(vesper_test_fixture_path_pattern)"
  matches="$(
    cd "$directory_path"
    find . -type f -print | sed 's#^\./##' | grep -Ei "$pattern" || true
  )"
  if [[ -n "$matches" ]]; then
    echo "Release directory contains test fixture resources: $directory_path" >&2
    printf '  %s\n' "$matches" >&2
    return 1
  fi
}

vesper_verify_binary_excludes_test_fixture_markers() {
  local binary_path="$1"
  local matches
  local pattern

  if [[ ! -f "$binary_path" ]]; then
    echo "Release binary does not exist: $binary_path" >&2
    return 1
  fi

  vesper_require_command strings
  pattern="$(vesper_test_fixture_binary_marker_pattern)"
  matches="$(strings "$binary_path" | grep -Ei "$pattern" || true)"
  if [[ -n "$matches" ]]; then
    echo "Release binary contains test fixture markers: $binary_path" >&2
    printf '  %s\n' "$matches" >&2
    return 1
  fi
}

vesper_verify_flutter_android_release_artifact() (
  set -euo pipefail

  local apk_path="$1"
  local binary_path
  local binary_count=0
  local temp_dir

  vesper_verify_archive_excludes_test_fixtures "$apk_path"

  temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/vesper-flutter-apk-verify.XXXXXX")"
  trap 'rm -rf "$temp_dir"' EXIT
  if ! unzip -qq "$apk_path" 'lib/*/libapp.so' -d "$temp_dir"; then
    echo "Flutter release APK does not contain libapp.so: $apk_path" >&2
    return 1
  fi

  while IFS= read -r -d '' binary_path; do
    binary_count=$((binary_count + 1))
    vesper_verify_binary_excludes_test_fixture_markers "$binary_path"
  done < <(find "$temp_dir" -type f -name libapp.so -print0)

  if [[ "$binary_count" -eq 0 ]]; then
    echo "Flutter release APK does not contain an extracted libapp.so: $apk_path" >&2
    return 1
  fi
)
