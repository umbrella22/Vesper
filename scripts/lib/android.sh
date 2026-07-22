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
  vesper_require_rust_targets Android "$@"
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

vesper_android_build_runtime_free_plugin() {
  local crate_name="$1"
  shift
  local output_dir="${1:-}"
  local build_profile="debug"
  local android_sdk_root
  local android_ndk_version
  local android_ndk_root
  local resolved_abis
  local selected_abis=()
  local required_targets=()
  local cargo_args
  local unexpected_runtime
  local abi

  if [[ -z "$output_dir" ]]; then
    echo "Usage: $0 <output-dir> [debug|release]" >&2
    echo "Android ABI selection is controlled by RUST_ANDROID_ABIS." >&2
    return 1
  fi
  shift

  if [[ $# -gt 0 && ( "$1" == "debug" || "$1" == "release" ) ]]; then
    build_profile="$1"
    shift
  fi
  if [[ $# -gt 0 ]]; then
    echo "Unexpected arguments: $*" >&2
    return 1
  fi

  android_sdk_root="$(vesper_android_sdk_root)"
  android_ndk_version="$(vesper_android_ndk_version)"
  android_ndk_root="${ANDROID_NDK_ROOT:-}"

  if ! resolved_abis="$(vesper_android_resolve_selected_abis)"; then
    return 1
  fi
  while IFS= read -r abi; do
    [[ -n "$abi" ]] && selected_abis+=("$abi")
  done <<<"$resolved_abis"
  for abi in "${selected_abis[@]}"; do
    required_targets+=("$(vesper_android_abi_to_rust_target "$abi")")
  done

  vesper_android_require_cargo_ndk "Android $crate_name plugins"
  vesper_android_require_rust_targets "${required_targets[@]}"
  if ! android_ndk_root="$(vesper_android_resolve_ndk_root "$android_sdk_root" "$android_ndk_root" "$android_ndk_version")"; then
    vesper_android_report_missing_ndk "$android_sdk_root" "$android_ndk_version"
    return 1
  fi

  rm -rf "$output_dir"
  mkdir -p "$output_dir"
  for abi in "${selected_abis[@]}"; do
    cargo_args=(ndk -o "$output_dir" -t "$abi" build -p "$crate_name")
    if [[ "$build_profile" == "release" ]]; then
      cargo_args+=(--release)
    fi
    cargo "${cargo_args[@]}"
  done

  unexpected_runtime="$(
    find "$output_dir" -type f \
      \( -name 'libav*.so' -o -name 'libsw*.so' -o -name 'libssl*.so' -o -name 'libcrypto*.so' -o -name 'libxml2*.so' \) \
      -print -quit
  )"
  if [[ -n "$unexpected_runtime" ]]; then
    echo "$crate_name must not bundle FFmpeg runtime libraries:" >&2
    echo "  $unexpected_runtime" >&2
    return 1
  fi

  echo
  echo "Built Android $crate_name plugin libraries into:"
  echo "  $output_dir"
}

vesper_android_resolve_gradle() {
  local project_dir="$1"
  local fallback_project_dir="${2:-}"
  local project_gradlew="$project_dir/gradlew"
  local local_gradle=""
  local fallback_gradle=""
  local wrapper_version=""
  local wrapper_properties="$project_dir/gradle/wrapper/gradle-wrapper.properties"

  if [[ -f "$wrapper_properties" ]]; then
    wrapper_version="$(sed -nE 's/^distributionUrl=.*gradle-([0-9][^-]*)-[^/]*\.zip.*/\1/p' "$wrapper_properties" | head -n 1)"
  fi

  if [[ "${CI:-}" == "true" ]]; then
    if command -v gradle >/dev/null 2>&1; then
      command -v gradle
      return 0
    fi

    echo "CI=true but no CI-provisioned gradle executable was found in PATH." >&2
    echo "Install Gradle with gradle/actions/setup-gradle or expose a CI-provisioned Gradle binary." >&2
    return 1
  fi

  if [[ -n "$wrapper_version" ]]; then
    local_gradle="$(find "$project_dir/.gradle/wrapper/dists" -path "*/gradle-$wrapper_version/bin/gradle" -type f -perm -111 2>/dev/null | sort | tail -n 1 || true)"
  else
    local_gradle="$(find "$project_dir/.gradle/wrapper/dists" -path '*/bin/gradle' -type f -perm -111 2>/dev/null | sort | tail -n 1 || true)"
  fi
  if [[ -n "$local_gradle" && -x "$local_gradle" ]]; then
    printf '%s\n' "$local_gradle"
    return 0
  fi

  if [[ -n "$fallback_project_dir" ]]; then
    if [[ -n "$wrapper_version" ]]; then
      fallback_gradle="$(find "$fallback_project_dir/.gradle/wrapper/dists" -path "*/gradle-$wrapper_version/bin/gradle" -type f -perm -111 2>/dev/null | sort | tail -n 1 || true)"
    else
      fallback_gradle="$(find "$fallback_project_dir/.gradle/wrapper/dists" -path '*/bin/gradle' -type f -perm -111 2>/dev/null | sort | tail -n 1 || true)"
    fi
    if [[ -n "$fallback_gradle" && -x "$fallback_gradle" ]]; then
      printf '%s\n' "$fallback_gradle"
      return 0
    fi
  fi

  cat <<EOF >&2
No local cached Gradle distribution was found for local Android work.

Project wrapper version:
  ${wrapper_version:-unknown}

Checked local distributions under:
  $project_dir/.gradle/wrapper/dists
EOF

  if [[ -n "$fallback_project_dir" ]]; then
    cat <<EOF >&2
  $fallback_project_dir/.gradle/wrapper/dists
EOF
  fi

  cat <<EOF >&2

Do not use gradlew for local agent work because it may download Gradle.
Seed the project-local wrapper cache, or run in CI with setup-gradle and CI=true.

Project wrapper intentionally not invoked:
  $project_gradlew
EOF
  return 1
}
