#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ios-framework.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
OUTPUT_DIR="$ROOT_DIR/dist/release/ios"
SCOPE="core"
FRAMEWORK_NAME="VesperPlayerKit"
FRAMEWORK_BUNDLE="$FRAMEWORK_NAME.framework"
XCFRAMEWORK_BUNDLE="$FRAMEWORK_NAME.xcframework"
BUNDLE_IDENTIFIER="io.github.ikaros.vesper.lib.ioshost"
PROJECT_INFO_PLIST="$ROOT_DIR/lib/ios/VesperPlayerKit/Sources/Generated-Info.plist"
EXPECTED_DEPLOYMENT_TARGET="$(vesper_apple_ios_deployment_target)"

usage() {
  cat <<EOF >&2
Usage: $0 [release-dir] [--scope core|complete]

Scopes:
  core      Verify the three VesperPlayerKit archives (default).
  complete  Verify the core archives and the complete optional plugin set.
EOF
}

if [[ $# -gt 0 && "$1" != --* ]]; then
  OUTPUT_DIR="$1"
  shift
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --scope)
      [[ -n "${2:-}" ]] || { echo "--scope requires a value." >&2; exit 1; }
      SCOPE="$2"
      shift 2
      ;;
    --scope=*)
      SCOPE="${1#*=}"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown iOS release verification option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

case "$SCOPE" in
  core|complete)
    ;;
  *)
    echo "Unsupported iOS release verification scope: $SCOPE" >&2
    usage
    exit 1
    ;;
esac

vesper_require_command cmp
vesper_require_command ditto
vesper_require_command diff
vesper_require_command lipo
vesper_require_command otool
vesper_require_command plutil
vesper_require_command rg
vesper_require_command ruby
vesper_require_command xcrun
vesper_require_command zipinfo

if [[ ! -f "$PROJECT_INFO_PLIST" ]]; then
  echo "Missing VesperPlayerKit release metadata: $PROJECT_INFO_PLIST" >&2
  exit 1
fi

EXPECTED_VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$PROJECT_INFO_PLIST" 2>/dev/null || true)"
EXPECTED_BUILD="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$PROJECT_INFO_PLIST" 2>/dev/null || true)"
if [[ -z "$EXPECTED_VERSION" || -z "$EXPECTED_BUILD" ]]; then
  echo "Unable to resolve VesperPlayerKit release version metadata." >&2
  exit 1
fi

DEVICE_ARCHIVE="$OUTPUT_DIR/VesperPlayerKit-ios-arm64.framework.zip"
SIMULATOR_ARCHIVE="$OUTPUT_DIR/VesperPlayerKit-ios-simulator-arm64.framework.zip"
XCFRAMEWORK_ARCHIVE="$OUTPUT_DIR/VesperPlayerKit.xcframework.zip"

for archive_path in "$DEVICE_ARCHIVE" "$SIMULATOR_ARCHIVE" "$XCFRAMEWORK_ARCHIVE"; do
  if [[ ! -f "$archive_path" || -L "$archive_path" ]]; then
    echo "Missing core iOS release archive: $archive_path" >&2
    exit 1
  fi
done

if find "$OUTPUT_DIR" -maxdepth 1 -type f -name '*.dylib*' -print -quit | grep -q .; then
  echo "iOS releases must not contain bare dylibs." >&2
  exit 1
fi

TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vesper-ios-core-release-verify.XXXXXX")"
trap 'rm -rf "$TEMP_DIR"' EXIT

validate_archive_entries() {
  local archive_path="$1"
  local expected_root="$2"
  local entry
  local duplicate_entry
  local entry_count=0

  duplicate_entry="$(zipinfo -1 "$archive_path" | sort | uniq -d | head -n 1)"
  if [[ -n "$duplicate_entry" ]]; then
    echo "Duplicate archive entry in $archive_path: $duplicate_entry" >&2
    return 1
  fi

  while IFS= read -r entry; do
    [[ -n "$entry" ]] || continue
    entry_count=$((entry_count + 1))
    case "$entry" in
      /*|..|../*|*/../*|*/..|__MACOSX|__MACOSX/*|._*|*/._*)
        echo "Unsafe or unexpected archive entry in $archive_path: $entry" >&2
        return 1
        ;;
    esac
    if [[ "$entry" != "$expected_root" && "$entry" != "$expected_root/" && "$entry" != "$expected_root/"* ]]; then
      echo "Unexpected top-level archive entry in $archive_path: $entry" >&2
      return 1
    fi
  done < <(zipinfo -1 "$archive_path")

  if [[ "$entry_count" -eq 0 ]]; then
    echo "Empty iOS release archive: $archive_path" >&2
    return 1
  fi

  if zipinfo -l "$archive_path" | awk '$1 ~ /^l/ { found = 1 } END { exit(found ? 0 : 1) }'; then
    echo "iOS release archives must not contain symlinks: $archive_path" >&2
    return 1
  fi
}

extract_single_root() {
  local archive_path="$1"
  local expected_root="$2"
  local destination_dir="$3"
  local expected_path="$destination_dir/$expected_root"
  local entries=()
  local entry

  if ! validate_archive_entries "$archive_path" "$expected_root"; then
    return 1
  fi
  mkdir -p "$destination_dir"
  if ! ditto -x -k "$archive_path" "$destination_dir"; then
    echo "Unable to extract iOS release archive: $archive_path" >&2
    return 1
  fi

  while IFS= read -r -d '' entry; do
    entries+=("$entry")
  done < <(find "$destination_dir" -mindepth 1 -maxdepth 1 -print0)

  if [[ ${#entries[@]} -ne 1 || "${entries[0]:-}" != "$expected_path" || ! -d "$expected_path" || -L "$expected_path" ]]; then
    echo "Release archive must contain exactly one $expected_root root: $archive_path" >&2
    return 1
  fi

  printf '%s\n' "$expected_path"
}

verify_interface_contract() {
  local framework_dir="$1"
  local target_suffix="$2"
  local module_dir="$framework_dir/Modules/$FRAMEWORK_NAME.swiftmodule"
  local public_interface="$module_dir/$target_suffix.swiftinterface"
  local private_interface="$module_dir/$target_suffix.private.swiftinterface"
  local interface_path

  for interface_path in "$public_interface" "$private_interface"; do
    if [[ ! -f "$interface_path" ]]; then
      echo "Missing VesperPlayerKit textual interface: $interface_path" >&2
      return 1
    fi
    if rg -n 'VesperPlayerKitBridgeShim|PlayerFfi|VesperRuntime|vesper_' "$interface_path"; then
      echo "VesperPlayerKit textual interface leaks private bridge declarations: $interface_path" >&2
      return 1
    fi
  done
}

verify_framework() {
  local framework_dir="$1"
  local expected_platform="$2"
  local target_suffix="$3"
  local binary_path="$framework_dir/$FRAMEWORK_NAME"
  local info_plist="$framework_dir/Info.plist"
  local actual_archs
  local actual_bundle_identifier
  local actual_version
  local actual_build
  local actual_minimum_os
  local macho_build
  local macho_minimum_os
  local dependencies

  vesper_ios_verify_flat_framework "$framework_dir" "$FRAMEWORK_NAME"
  vesper_ios_verify_framework_platform "$framework_dir" "$FRAMEWORK_NAME" "$expected_platform"

  actual_archs="$(lipo -archs "$binary_path")"
  if [[ "$actual_archs" != "arm64" ]]; then
    echo "VesperPlayerKit release slices must contain only arm64: $binary_path ($actual_archs)" >&2
    return 1
  fi

  actual_bundle_identifier="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$info_plist" 2>/dev/null || true)"
  actual_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$info_plist" 2>/dev/null || true)"
  actual_build="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$info_plist" 2>/dev/null || true)"
  actual_minimum_os="$(/usr/libexec/PlistBuddy -c 'Print :MinimumOSVersion' "$info_plist" 2>/dev/null || true)"
  if ! plutil -extract CFBundleSupportedPlatforms json -o - "$info_plist" | \
    ruby -rjson -e 'exit(JSON.parse(STDIN.read) == [ARGV.fetch(0)] ? 0 : 1)' "$expected_platform"
  then
    echo "VesperPlayerKit must declare exactly one supported platform ($expected_platform): $info_plist" >&2
    return 1
  fi
  if [[ "$actual_bundle_identifier" != "$BUNDLE_IDENTIFIER" || "$actual_version" != "$EXPECTED_VERSION" || "$actual_build" != "$EXPECTED_BUILD" ]]; then
    echo "Unexpected VesperPlayerKit bundle metadata: $framework_dir" >&2
    echo "  bundle identifier: ${actual_bundle_identifier:-<missing>} (expected $BUNDLE_IDENTIFIER)" >&2
    echo "  version:           ${actual_version:-<missing>} (expected $EXPECTED_VERSION)" >&2
    echo "  build:             ${actual_build:-<missing>} (expected $EXPECTED_BUILD)" >&2
    return 1
  fi
  if [[ "$actual_minimum_os" != "$EXPECTED_DEPLOYMENT_TARGET" ]]; then
    echo "Unexpected VesperPlayerKit minimum OS in $info_plist: ${actual_minimum_os:-<missing>} (expected $EXPECTED_DEPLOYMENT_TARGET)" >&2
    return 1
  fi

  macho_build="$(xcrun vtool -show-build "$binary_path")"
  macho_minimum_os="$(printf '%s\n' "$macho_build" | awk '$1 == "minos" { print $2; exit }')"
  if [[ "$macho_minimum_os" != "$EXPECTED_DEPLOYMENT_TARGET" ]]; then
    echo "Unexpected VesperPlayerKit Mach-O minimum OS: $binary_path" >&2
    echo "  actual:   ${macho_minimum_os:-<missing>}" >&2
    echo "  expected: $EXPECTED_DEPLOYMENT_TARGET" >&2
    return 1
  fi

  dependencies="$(vesper_ios_binary_dependencies "$binary_path")"
  vesper_ios_verify_exact_dynamic_dependency_list \
    "$binary_path" \
    "$FRAMEWORK_NAME" \
    "$dependencies"
  verify_interface_contract "$framework_dir" "$target_suffix"
}

write_consumer_probe() {
  local output_path="$1"
  printf '%s\n' \
    'import VesperPlayerKit' \
    '' \
    '@MainActor' \
    'public func makePlayer() {' \
    '    _ = VesperPlayerControllerFactory.makeDefault()' \
    '}' \
    >"$output_path"
}

verify_consumer() {
  local framework_dir="$1"
  local sdk="$2"
  local target="$3"
  local label="$4"
  local probe_root="$TEMP_DIR/consumer-$label"
  local probe_framework="$probe_root/$FRAMEWORK_BUNDLE"
  local module_cache="$probe_root/module-cache"
  local probe_source="$probe_root/Consumer.swift"
  local probe_binary="$probe_root/libVesperPlayerKitConsumer.dylib"
  local sdk_path

  mkdir -p "$probe_root" "$module_cache"
  ditto "$framework_dir" "$probe_framework"
  find "$probe_framework/Modules" -type f -name '*.swiftmodule' -delete
  write_consumer_probe "$probe_source"
  sdk_path="$(xcrun --sdk "$sdk" --show-sdk-path)"

  xcrun --sdk "$sdk" swiftc \
    -swift-version 5 \
    -target "$target" \
    -sdk "$sdk_path" \
    -module-cache-path "$module_cache" \
    -F "$probe_root" \
    -framework "$FRAMEWORK_NAME" \
    -emit-library \
    "$probe_source" \
    -o "$probe_binary"

  if ! otool -L "$probe_binary" | grep -Fq "@rpath/$FRAMEWORK_BUNDLE/$FRAMEWORK_NAME"; then
    echo "Consumer smoke did not link VesperPlayerKit: $label" >&2
    return 1
  fi
  if otool -L "$probe_binary" | grep -Fq 'VesperPlayerKitBridgeShim'; then
    echo "Consumer smoke links the private BridgeShim module: $label" >&2
    return 1
  fi
}

DEVICE_FRAMEWORK="$(extract_single_root "$DEVICE_ARCHIVE" "$FRAMEWORK_BUNDLE" "$TEMP_DIR/device-archive")"
SIMULATOR_FRAMEWORK="$(extract_single_root "$SIMULATOR_ARCHIVE" "$FRAMEWORK_BUNDLE" "$TEMP_DIR/simulator-archive")"
XCFRAMEWORK_PATH="$(extract_single_root "$XCFRAMEWORK_ARCHIVE" "$XCFRAMEWORK_BUNDLE" "$TEMP_DIR/xcframework-archive")"

vesper_ios_verify_xcframework_manifest "$XCFRAMEWORK_PATH" "$FRAMEWORK_NAME"
XC_DEVICE_FRAMEWORK="$(vesper_ios_xcframework_slice_framework "$XCFRAMEWORK_PATH" "$FRAMEWORK_NAME" iphoneos)"
XC_SIMULATOR_FRAMEWORK="$(vesper_ios_xcframework_slice_framework "$XCFRAMEWORK_PATH" "$FRAMEWORK_NAME" iphonesimulator)"

verify_framework "$DEVICE_FRAMEWORK" iPhoneOS arm64-apple-ios
verify_framework "$SIMULATOR_FRAMEWORK" iPhoneSimulator arm64-apple-ios-simulator
verify_framework "$XC_DEVICE_FRAMEWORK" iPhoneOS arm64-apple-ios
verify_framework "$XC_SIMULATOR_FRAMEWORK" iPhoneSimulator arm64-apple-ios-simulator

if ! diff -qr "$DEVICE_FRAMEWORK" "$XC_DEVICE_FRAMEWORK"; then
  echo "Standalone device framework differs from the XCFramework device slice." >&2
  exit 1
fi
if ! diff -qr "$SIMULATOR_FRAMEWORK" "$XC_SIMULATOR_FRAMEWORK"; then
  echo "Standalone Simulator framework differs from the XCFramework Simulator slice." >&2
  exit 1
fi

verify_consumer \
  "$DEVICE_FRAMEWORK" \
  iphoneos \
  "$(vesper_apple_slice_clang_target ios-arm64 "$EXPECTED_DEPLOYMENT_TARGET")" \
  standalone-device
verify_consumer \
  "$SIMULATOR_FRAMEWORK" \
  iphonesimulator \
  "$(vesper_apple_slice_clang_target ios-simulator-arm64 "$EXPECTED_DEPLOYMENT_TARGET")" \
  standalone-simulator
verify_consumer \
  "$XC_DEVICE_FRAMEWORK" \
  iphoneos \
  "$(vesper_apple_slice_clang_target ios-arm64 "$EXPECTED_DEPLOYMENT_TARGET")" \
  xcframework-device
verify_consumer \
  "$XC_SIMULATOR_FRAMEWORK" \
  iphonesimulator \
  "$(vesper_apple_slice_clang_target ios-simulator-arm64 "$EXPECTED_DEPLOYMENT_TARGET")" \
  xcframework-simulator

if [[ "$SCOPE" == "complete" ]]; then
  "$ROOT_DIR/scripts/ios/verify-player-optional-plugins-release.sh" "$OUTPUT_DIR"
fi

echo "Verified VesperPlayerKit iOS release assets ($SCOPE):"
echo "  $OUTPUT_DIR"
