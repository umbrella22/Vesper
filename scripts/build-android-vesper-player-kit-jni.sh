#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
LIB_DIR="$ROOT_DIR/lib/android/vesper-player-kit"
JNI_LIBS_DIR="$LIB_DIR/src/main/jniLibs"
PROFILE="${1:-debug}"
shift || true
ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}"
ANDROID_NDK_VERSION="29.0.14206865"
ANDROID_NDK_ROOT="${ANDROID_NDK_ROOT:-}"
# Android 侧 JNI / AAR 分发统一收敛为 arm64-only。
DEFAULT_ABIS=(
  "arm64-v8a"
)

resolve_selected_abis() {
  local -a resolved=()
  local token

  if [[ $# -gt 0 ]]; then
    resolved=("$@")
  elif [[ -n "${RUST_ANDROID_ABIS:-}" ]]; then
    # Support both comma-separated and space-separated ABI lists.
    read -r -a resolved <<<"${RUST_ANDROID_ABIS//,/ }"
  else
    resolved=("${DEFAULT_ABIS[@]}")
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

map_abi_to_rust_target() {
  case "$1" in
    arm64-v8a)
      echo "aarch64-linux-android"
      ;;
    *)
      return 1
      ;;
  esac
}

selected_abis=()
while IFS= read -r abi; do
  selected_abis+=("$abi")
done < <(resolve_selected_abis "$@")

REQUIRED_TARGETS=()
for abi in "${selected_abis[@]}"; do
  REQUIRED_TARGETS+=("$(map_abi_to_rust_target "$abi")")
done

installed_targets="$(rustup target list --installed)"

if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "cargo-ndk is required to build Android JNI libraries." >&2
  echo "Install it with: cargo install cargo-ndk" >&2
  exit 1
fi

missing_targets=()
for target in "${REQUIRED_TARGETS[@]}"; do
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

resolve_ndk_root() {
  local candidate

  if [[ -n "$ANDROID_NDK_ROOT" ]]; then
    echo "$ANDROID_NDK_ROOT"
    return 0
  fi

  candidate="$ANDROID_SDK_ROOT/ndk/$ANDROID_NDK_VERSION"
  if [[ -f "$candidate/source.properties" ]]; then
    echo "$candidate"
    return 0
  fi

  if [[ -d "$ANDROID_SDK_ROOT/ndk" ]]; then
    while IFS= read -r candidate; do
      if [[ -f "$candidate/source.properties" ]]; then
        echo "$candidate"
        return 0
      fi
    done < <(find "$ANDROID_SDK_ROOT/ndk" -mindepth 1 -maxdepth 1 -type d | sort -Vr)
  fi

  return 1
}

if ! ANDROID_NDK_ROOT="$(resolve_ndk_root)"; then
  echo "Android NDK is missing or incomplete at:" >&2
  echo "  $ANDROID_SDK_ROOT/ndk/$ANDROID_NDK_VERSION" >&2
  echo >&2
  echo "Expected a complete NDK installation containing:" >&2
  echo "  <ndk-dir>/source.properties" >&2
  echo >&2
  echo "Install Android NDK $ANDROID_NDK_VERSION from Android Studio:" >&2
  echo "  Settings > Languages & Frameworks > Android SDK > SDK Tools > NDK (Side by side)" >&2
  echo >&2
  echo "If Android Studio installed a different NDK version, set ANDROID_NDK_ROOT before running this script." >&2
  exit 1
fi

rm -rf "$JNI_LIBS_DIR"
mkdir -p "$JNI_LIBS_DIR"

BUILD_FLAGS=()
if [[ "$PROFILE" == "release" ]]; then
  BUILD_FLAGS+=(--release)
fi

NDK_TARGET_ARGS=()
for abi in "${selected_abis[@]}"; do
  NDK_TARGET_ARGS+=(-t "$abi")
done

if [[ ${#BUILD_FLAGS[@]} -gt 0 ]]; then
  cargo ndk \
    -o "$JNI_LIBS_DIR" \
    "${NDK_TARGET_ARGS[@]}" \
    build \
    -p player-jni-android \
    "${BUILD_FLAGS[@]}"
else
  cargo ndk \
    -o "$JNI_LIBS_DIR" \
    "${NDK_TARGET_ARGS[@]}" \
    build \
    -p player-jni-android
fi

echo
echo "Built Android JNI libraries into:"
echo "  $JNI_LIBS_DIR"
echo "Selected Android ABIs:"
for abi in "${selected_abis[@]}"; do
  echo "  $abi"
done
