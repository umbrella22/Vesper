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
  vesper_require_rust_targets Apple "$@"
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

vesper_apple_platform_sdk_name() {
  case "$1" in
    iPhoneOS)
      echo "iphoneos"
      ;;
    iPhoneSimulator)
      echo "iphonesimulator"
      ;;
    *)
      return 1
      ;;
  esac
}

vesper_apple_xcode_actual_version() {
  local xcode_version="$1"
  local major
  local minor

  major="${xcode_version%%.*}"
  if [[ "$xcode_version" == *.* ]]; then
    minor="${xcode_version#*.}"
    minor="${minor%%.*}"
  else
    minor="0"
  fi

  printf '%s%s0\n' "$major" "$minor"
}

vesper_apple_add_plist_string() {
  local output_path="$1"
  local key="$2"
  local value="$3"

  [[ -n "$value" ]] || return 0
  /usr/libexec/PlistBuddy -c "Delete :$key" "$output_path" >/dev/null 2>&1 || true
  /usr/libexec/PlistBuddy -c "Add :$key string $value" "$output_path"
}

vesper_apple_add_framework_install_metadata() {
  local output_path="$1"
  local platform_name="$2"
  local sdk_name
  local sdk_version
  local sdk_build
  local xcode_version
  local xcode_build

  sdk_name="$(vesper_apple_platform_sdk_name "$platform_name")"
  sdk_version="$(xcodebuild -sdk "$sdk_name" -version SDKVersion 2>/dev/null || true)"
  sdk_build="$(xcodebuild -sdk "$sdk_name" -version ProductBuildVersion 2>/dev/null || true)"
  xcode_version="$(xcodebuild -version 2>/dev/null | awk '/^Xcode / {print $2; exit}' || true)"
  xcode_build="$(xcodebuild -version 2>/dev/null | awk '/^Build version / {print $3; exit}' || true)"

  vesper_apple_add_plist_string "$output_path" BuildMachineOSBuild "$(sw_vers -buildVersion 2>/dev/null || true)"
  vesper_apple_add_plist_string "$output_path" DTCompiler "com.apple.compilers.llvm.clang.1_0"
  vesper_apple_add_plist_string "$output_path" DTPlatformBuild "$sdk_build"
  vesper_apple_add_plist_string "$output_path" DTPlatformName "$sdk_name"
  vesper_apple_add_plist_string "$output_path" DTPlatformVersion "$sdk_version"
  vesper_apple_add_plist_string "$output_path" DTSDKBuild "$sdk_build"
  vesper_apple_add_plist_string "$output_path" DTSDKName "${sdk_name}${sdk_version}"
  if [[ -n "$xcode_version" ]]; then
    vesper_apple_add_plist_string "$output_path" DTXcode "$(vesper_apple_xcode_actual_version "$xcode_version")"
  fi
  vesper_apple_add_plist_string "$output_path" DTXcodeBuild "$xcode_build"

  /usr/libexec/PlistBuddy -c "Delete :UIDeviceFamily" "$output_path" >/dev/null 2>&1 || true
  /usr/libexec/PlistBuddy -c "Add :UIDeviceFamily array" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :UIDeviceFamily:0 integer 1" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :UIDeviceFamily:1 integer 2" "$output_path"
}
