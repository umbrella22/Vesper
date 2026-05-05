if [[ -n "${VESPER_DESKTOP_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_DESKTOP_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

vesper_desktop_shared_library_name() {
  local library_stem="$1"

  case "$(uname -s)" in
    Darwin)
      echo "lib${library_stem}.dylib"
      ;;
    Linux)
      echo "lib${library_stem}.so"
      ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "${library_stem}.dll"
      ;;
    *)
      echo "Unsupported platform: $(uname -s)" >&2
      exit 1
      ;;
  esac
}

vesper_desktop_normalize_runtime_path() {
  local path="$1"

  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
      if command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$path"
      else
        printf '%s\n' "$path"
      fi
      ;;
    *)
      printf '%s\n' "$path"
      ;;
  esac
}

vesper_desktop_target_dir() {
  if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    if [[ "$CARGO_TARGET_DIR" = /* ]]; then
      printf '%s\n' "$CARGO_TARGET_DIR"
    else
      printf '%s\n' "$VESPER_REPO_ROOT/$CARGO_TARGET_DIR"
    fi
    return 0
  fi

  printf '%s\n' "$VESPER_REPO_ROOT/target"
}

vesper_desktop_resolve_plugin_path() {
  local library_name="$1"
  local target_dir="$2"
  local profile="$3"
  local override_path="$4"
  local override_env_name="$5"
  local crate_label="$6"
  local candidate

  if [[ -n "$override_path" ]]; then
    if [[ ! -f "$override_path" ]]; then
      echo "$override_env_name points to a missing file: $override_path" >&2
      exit 1
    fi
    printf '%s\n' "$override_path"
    return 0
  fi

  for candidate in \
    "$target_dir/$profile/$library_name" \
    "$target_dir/$profile/deps/$library_name" \
    "$target_dir/debug/$library_name" \
    "$target_dir/debug/deps/$library_name" \
    "$target_dir/release/$library_name" \
    "$target_dir/release/deps/$library_name"; do
    if [[ -f "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  echo "Could not find $library_name under $target_dir; build $crate_label first or set $override_env_name." >&2
  exit 1
}

vesper_desktop_is_ci_environment() {
  [[ "${CI:-}" == "true" || -n "${GITHUB_ACTIONS:-}" ]]
}
