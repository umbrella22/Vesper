if [[ -n "${VESPER_APPLE_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_APPLE_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

VESPER_APPLE_DEFAULT_SLICES=(
  "ios-arm64"
  "ios-simulator-arm64"
)

vesper_apple_ios_deployment_target() {
  printf '%s\n' "${VESPER_APPLE_IOS_DEPLOYMENT_TARGET:-17.0}"
}

vesper_apple_resolve_selected_slices() {
  local -a resolved=()
  local token

  if [[ $# -gt 0 ]]; then
    resolved=("$@")
  else
    resolved=("${VESPER_APPLE_DEFAULT_SLICES[@]}")
  fi

  if [[ ${#resolved[@]} -eq 0 ]]; then
    echo "No Apple slices were selected." >&2
    exit 1
  fi

  for token in "${resolved[@]}"; do
    case "$token" in
      ios-arm64|ios-simulator-arm64)
        ;;
      *)
        echo "Unsupported Apple slice: $token" >&2
        echo "Supported slices: ios-arm64, ios-simulator-arm64" >&2
        exit 1
        ;;
    esac
  done

  printf '%s\n' "${resolved[@]}"
}

vesper_apple_slice_sdk() {
  case "$1" in
    ios-arm64)
      echo "iphoneos"
      ;;
    ios-simulator-arm64)
      echo "iphonesimulator"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_apple_slice_arch() {
  case "$1" in
    ios-arm64|ios-simulator-arm64)
      echo "arm64"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_apple_slice_clang_target() {
  local slice="$1"
  local deployment_target="${2:-$(vesper_apple_ios_deployment_target)}"

  case "$slice" in
    ios-arm64)
      echo "arm64-apple-ios${deployment_target}"
      ;;
    ios-simulator-arm64)
      echo "arm64-apple-ios${deployment_target}-simulator"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_apple_slice_output_root() {
  local slice="$1"
  local ffmpeg_output_dir="$2"

  case "$slice" in
    ios-arm64)
      echo "$ffmpeg_output_dir/ios"
      ;;
    ios-simulator-arm64)
      echo "$ffmpeg_output_dir/ios-simulator"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_apple_slice_output_libdir() {
  case "$1" in
    ios-arm64|ios-simulator-arm64)
      echo "arm64"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_ios_slice_rust_target() {
  case "$1" in
    ios-arm64)
      echo "aarch64-apple-ios"
      ;;
    ios-simulator-arm64)
      echo "aarch64-apple-ios-sim"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_apple_require_rust_targets() {
  local installed_targets
  local target
  local -a missing_targets=()

  installed_targets="$(rustup target list --installed)"
  for target in "$@"; do
    if [[ "$installed_targets" != *"$target"* ]]; then
      missing_targets+=("$target")
    fi
  done

  if [[ ${#missing_targets[@]} -gt 0 ]]; then
    echo "Required Rust Apple targets are missing:" >&2
    for target in "${missing_targets[@]}"; do
      echo "  $target" >&2
    done
    echo >&2
    echo "Install them with:" >&2
    echo "  rustup target add ${missing_targets[*]}" >&2
    exit 1
  fi
}

vesper_apple_extract_libxml2_version() {
  local sdk_path="$1"
  local header_path="$sdk_path/usr/include/libxml2/libxml/xmlversion.h"

  if [[ ! -f "$header_path" ]]; then
    echo "2.0.0"
    return 0
  fi

  sed -n 's/^#define LIBXML_DOTTED_VERSION "\(.*\)"$/\1/p' "$header_path" | head -n 1
}
