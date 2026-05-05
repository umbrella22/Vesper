if [[ -n "${VESPER_ANDROID_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_ANDROID_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/common.sh"

VESPER_ANDROID_NDK_VERSION_DEFAULT="29.0.14206865"
VESPER_ANDROID_DEFAULT_ABIS=(
  "arm64-v8a"
)

vesper_android_sdk_root() {
  printf '%s\n' "${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
}

vesper_android_ndk_version() {
  printf '%s\n' "${ANDROID_NDK_VERSION:-${VESPER_ANDROID_NDK_VERSION:-$VESPER_ANDROID_NDK_VERSION_DEFAULT}}"
}

vesper_android_resolve_selected_abis() {
  local -a resolved=()
  local token

  if [[ $# -gt 0 ]]; then
    resolved=("$@")
  elif [[ -n "${RUST_ANDROID_ABIS:-}" ]]; then
    read -r -a resolved <<<"${RUST_ANDROID_ABIS//,/ }"
  else
    resolved=("${VESPER_ANDROID_DEFAULT_ABIS[@]}")
  fi

  if [[ ${#resolved[@]} -eq 0 ]]; then
    echo "No Android ABIs were selected." >&2
    exit 1
  fi

  for token in "${resolved[@]}"; do
    case "$token" in
      arm64-v8a)
        ;;
      *)
        echo "Unsupported Android ABI: $token" >&2
        echo "Supported ABIs: arm64-v8a" >&2
        exit 1
        ;;
    esac
  done

  printf '%s\n' "${resolved[@]}"
}

vesper_android_abi_to_rust_target() {
  case "$1" in
    arm64-v8a)
      echo "aarch64-linux-android"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_android_abi_to_ffmpeg_arch() {
  case "$1" in
    arm64-v8a)
      echo "aarch64"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_android_abi_to_ffmpeg_cpu() {
  case "$1" in
    arm64-v8a)
      echo "armv8-a"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_android_abi_to_openssl_target() {
  case "$1" in
    arm64-v8a)
      echo "android-arm64"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_android_collect_rust_targets() {
  local abi
  for abi in "$@"; do
    vesper_android_abi_to_rust_target "$abi"
  done
}

vesper_android_require_rust_targets() {
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
    echo "Required Rust Android targets are missing:" >&2
    for target in "${missing_targets[@]}"; do
      echo "  $target" >&2
    done
    echo >&2
    echo "Install them with:" >&2
    echo "  rustup target add ${missing_targets[*]}" >&2
    exit 1
  fi
}

vesper_android_resolve_ndk_root() {
  local sdk_root="$1"
  local ndk_root="${2:-}"
  local ndk_version="${3:-$(vesper_android_ndk_version)}"
  local candidate

  if [[ -n "$ndk_root" ]]; then
    echo "$ndk_root"
    return 0
  fi

  candidate="$sdk_root/ndk/$ndk_version"
  if [[ -f "$candidate/source.properties" ]]; then
    echo "$candidate"
    return 0
  fi

  if [[ -d "$sdk_root/ndk" ]]; then
    local ndk_dirs

    if ! ndk_dirs="$(find "$sdk_root/ndk" -mindepth 1 -maxdepth 1 -type d | sort -Vr 2>/dev/null)"; then
      ndk_dirs="$(find "$sdk_root/ndk" -mindepth 1 -maxdepth 1 -type d | sort -r)"
    fi

    while IFS= read -r candidate; do
      [[ -n "$candidate" ]] || continue
      if [[ -f "$candidate/source.properties" ]]; then
        echo "$candidate"
        return 0
      fi
    done <<<"$ndk_dirs"
  fi

  return 1
}

vesper_android_resolve_host_tag() {
  local ndk_root="$1"
  local os
  local arch

  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin)
      if [[ "$arch" == "arm64" ]]; then
        if [[ -d "$ndk_root/toolchains/llvm/prebuilt/darwin-arm64" ]]; then
          echo "darwin-arm64"
          return 0
        fi
      fi
      echo "darwin-x86_64"
      ;;
    Linux)
      echo "linux-x86_64"
      ;;
    *)
      echo "Unsupported host OS: $os" >&2
      return 1
      ;;
  esac
}

vesper_android_require_cargo_ndk() {
  local description="$1"

  if ! command -v cargo-ndk >/dev/null 2>&1; then
    echo "cargo-ndk is required to build $description." >&2
    echo "Install it with: cargo install cargo-ndk" >&2
    exit 1
  fi
}

vesper_android_report_missing_ndk() {
  local sdk_root="$1"
  local ndk_version="${2:-$(vesper_android_ndk_version)}"
  local suffix="${3:-Install Android NDK $ndk_version from Android Studio.}"

  echo "Android NDK is missing or incomplete at:" >&2
  echo "  $sdk_root/ndk/$ndk_version" >&2
  echo >&2
  echo "Expected a complete NDK installation containing:" >&2
  echo "  <ndk-dir>/source.properties" >&2
  echo >&2
  echo "$suffix" >&2
}
