if [[ -n "${VESPER_IOS_FRAMEWORK_SH_INCLUDED:-}" ]]; then
  return 0 2>/dev/null || exit 0
fi
VESPER_IOS_FRAMEWORK_SH_INCLUDED=1

source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/apple.sh"

vesper_ios_ffmpeg_framework_name() {
  case "$1" in
    avcodec) echo "VesperFFmpegAVCodec" ;;
    avdevice) echo "VesperFFmpegAVDevice" ;;
    avfilter) echo "VesperFFmpegAVFilter" ;;
    avformat) echo "VesperFFmpegAVFormat" ;;
    avutil) echo "VesperFFmpegAVUtil" ;;
    postproc) echo "VesperFFmpegPostproc" ;;
    swresample) echo "VesperFFmpegSWResample" ;;
    swscale) echo "VesperFFmpegSWScale" ;;
    *)
      echo "Unsupported iOS FFmpeg framework component: $1" >&2
      return 1
      ;;
  esac
}

vesper_ios_ffmpeg_bundle_identifier() {
  case "$1" in
    avcodec) echo "io.github.ikaros.vesper.ffmpeg.avcodec" ;;
    avdevice) echo "io.github.ikaros.vesper.ffmpeg.avdevice" ;;
    avfilter) echo "io.github.ikaros.vesper.ffmpeg.avfilter" ;;
    avformat) echo "io.github.ikaros.vesper.ffmpeg.avformat" ;;
    avutil) echo "io.github.ikaros.vesper.ffmpeg.avutil" ;;
    postproc) echo "io.github.ikaros.vesper.ffmpeg.postproc" ;;
    swresample) echo "io.github.ikaros.vesper.ffmpeg.swresample" ;;
    swscale) echo "io.github.ikaros.vesper.ffmpeg.swscale" ;;
    *)
      echo "Unsupported iOS FFmpeg framework component: $1" >&2
      return 1
      ;;
  esac
}

vesper_ios_ffmpeg_library_from_dependency() {
  local dependency_name

  dependency_name="$(basename "$1")"
  case "$dependency_name" in
    libavcodec*.dylib) echo "avcodec" ;;
    libavdevice*.dylib) echo "avdevice" ;;
    libavfilter*.dylib) echo "avfilter" ;;
    libavformat*.dylib) echo "avformat" ;;
    libavutil*.dylib) echo "avutil" ;;
    libpostproc*.dylib) echo "postproc" ;;
    libswresample*.dylib) echo "swresample" ;;
    libswscale*.dylib) echo "swscale" ;;
    *) return 1 ;;
  esac
}

vesper_ios_framework_info_plist() {
  local output_path="$1"
  local framework_name="$2"
  local bundle_identifier="$3"
  local platform_name="$4"
  local minimum_os_version="$5"
  local version="$6"
  local build="$7"

  /usr/libexec/PlistBuddy -c "Clear dict" "$output_path" >/dev/null 2>&1 || true
  /usr/libexec/PlistBuddy -c "Add :CFBundleDevelopmentRegion string en" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleExecutable string $framework_name" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string $bundle_identifier" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleInfoDictionaryVersion string 6.0" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleName string $framework_name" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundlePackageType string FMWK" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $version" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms array" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms:0 string $platform_name" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :CFBundleVersion string $build" "$output_path"
  /usr/libexec/PlistBuddy -c "Add :MinimumOSVersion string $minimum_os_version" "$output_path"
  vesper_apple_add_framework_install_metadata "$output_path" "$platform_name"
}

vesper_ios_write_binary_framework_module() {
  local framework_dir="$1"
  local framework_name="$2"

  printf '%s\n' \
    "/* Binary distribution marker for $framework_name. */" \
    >"$framework_dir/Headers/$framework_name.h"
  printf '%s\n' \
    "framework module $framework_name {" \
    "  umbrella header \"$framework_name.h\"" \
    '  export *' \
    '  module * { export * }' \
    '}' \
    >"$framework_dir/Modules/module.modulemap"
}

vesper_ios_parse_otool_dependency_paths() {
  awk '
    NR > 1 {
      dependency = $0
      sub(/^[[:space:]]*/, "", dependency)
      sub(/[[:space:]]+\(compatibility version .*$/, "", dependency)
      print dependency
    }
  '
}

vesper_ios_binary_dependencies() {
  otool -L "$1" | vesper_ios_parse_otool_dependency_paths
}

vesper_ios_verify_exact_dynamic_dependency_list() {
  local binary_path="$1"
  local framework_name="$2"
  local dependencies="$3"
  shift 3
  local expected_components=()
  local component_name
  local expected_dependency
  local actual_dependency
  local dependency_found
  local dependency_is_expected

  while [[ $# -gt 0 ]]; do
    expected_components+=("$1")
    shift
  done

  for component_name in ${expected_components[@]+"${expected_components[@]}"}; do
    expected_dependency="@rpath/$component_name.framework/$component_name"
    dependency_found=false
    while IFS= read -r actual_dependency; do
      if [[ "$actual_dependency" == "$expected_dependency" ]]; then
        dependency_found=true
        break
      fi
    done <<<"$dependencies"
    if [[ "$dependency_found" != "true" ]]; then
      echo "$binary_path is missing dynamic dependency $expected_dependency" >&2
      return 1
    fi
  done

  while IFS= read -r actual_dependency; do
    [[ -n "$actual_dependency" ]] || continue
    if [[ "$actual_dependency" == "@rpath/$framework_name.framework/$framework_name" ]]; then
      continue
    fi

    dependency_is_expected=false
    for component_name in ${expected_components[@]+"${expected_components[@]}"}; do
      expected_dependency="@rpath/$component_name.framework/$component_name"
      if [[ "$actual_dependency" == "$expected_dependency" ]]; then
        dependency_is_expected=true
        break
      fi
    done
    if [[ "$dependency_is_expected" == "true" ]]; then
      continue
    fi

    case "$actual_dependency" in
      /usr/lib/*|/System/Library/Frameworks/*)
        continue
        ;;
    esac

    echo "$binary_path has unexpected non-system dynamic dependency:" >&2
    echo "  $actual_dependency" >&2
    return 1
  done <<<"$dependencies"
}

vesper_ios_verify_optional_framework_dependencies() {
  local binary_path="$1"
  local framework_name="$2"

  case "$framework_name" in
    VesperFFmpegAVCodec)
      vesper_ios_verify_exact_framework_dependencies \
        "$binary_path" \
        "$framework_name" \
        VesperFFmpegAVUtil
      ;;
    VesperFFmpegAVFormat)
      vesper_ios_verify_exact_framework_dependencies \
        "$binary_path" \
        "$framework_name" \
        VesperFFmpegAVCodec \
        VesperFFmpegAVUtil
      ;;
    VesperPlayerRemuxFfmpegPlugin|VesperPlayerSourceNormalizerFfmpegPlugin)
      vesper_ios_verify_exact_framework_dependencies \
        "$binary_path" \
        "$framework_name" \
        VesperFFmpegAVCodec \
        VesperFFmpegAVFormat \
        VesperFFmpegAVUtil
      ;;
    *)
      vesper_ios_verify_exact_framework_dependencies \
        "$binary_path" \
        "$framework_name"
      ;;
  esac
}

vesper_ios_verify_exact_framework_dependencies() {
  local binary_path="$1"
  local framework_name="$2"
  shift 2
  local dependencies

  dependencies="$(vesper_ios_binary_dependencies "$binary_path")"
  vesper_ios_verify_exact_dynamic_dependency_list \
    "$binary_path" \
    "$framework_name" \
    "$dependencies" \
    "$@"
}

vesper_ios_ensure_rpath() {
  local binary_path="$1"
  local rpath="$2"

  if ! otool -l "$binary_path" | awk -v expected="$rpath" '$1 == "path" && $2 == expected { found = 1 } END { exit !found }'; then
    install_name_tool -add_rpath "$rpath" "$binary_path"
  fi
}

vesper_ios_remove_rpath() {
  local binary_path="$1"
  local rpath="$2"

  if otool -l "$binary_path" | awk -v expected="$rpath" '$1 == "path" && $2 == expected { found = 1 } END { exit !found }'; then
    install_name_tool -delete_rpath "$rpath" "$binary_path"
  fi
}

vesper_ios_rewrite_ffmpeg_dependencies() {
  local binary_path="$1"
  local dependency
  local library_name
  local framework_name

  while IFS= read -r dependency; do
    if ! library_name="$(vesper_ios_ffmpeg_library_from_dependency "$dependency")"; then
      continue
    fi
    framework_name="$(vesper_ios_ffmpeg_framework_name "$library_name")"
    install_name_tool -change \
      "$dependency" \
      "@rpath/$framework_name.framework/$framework_name" \
      "$binary_path"
  done < <(vesper_ios_binary_dependencies "$binary_path")
}

vesper_ios_prepare_framework_binary() {
  local binary_path="$1"
  local framework_name="$2"

  install_name_tool -id \
    "@rpath/$framework_name.framework/$framework_name" \
    "$binary_path"
  vesper_ios_remove_rpath \
    "$binary_path" \
    "@loader_path/VesperPlayerFfmpegRuntime.framework/Frameworks"
  vesper_ios_remove_rpath \
    "$binary_path" \
    "@loader_path/../VesperPlayerFfmpegRuntime.framework/Frameworks"
  vesper_ios_remove_rpath "$binary_path" "@loader_path/Frameworks"
  vesper_ios_ensure_rpath "$binary_path" "@loader_path/.."
  vesper_ios_rewrite_ffmpeg_dependencies "$binary_path"
}

vesper_ios_verify_framework_binary_dependencies() {
  local binary_path="$1"
  local dependency
  local library_name

  while IFS= read -r dependency; do
    if library_name="$(vesper_ios_ffmpeg_library_from_dependency "$dependency")"; then
      echo "Unwrapped FFmpeg dependency remains in $binary_path:" >&2
      echo "  $dependency ($library_name)" >&2
      return 1
    fi
  done < <(vesper_ios_binary_dependencies "$binary_path")
}

vesper_ios_verify_framework_install_name() {
  local binary_path="$1"
  local framework_name="$2"
  local install_name
  local expected_install_name

  install_name="$(otool -D "$binary_path" | awk 'NR == 2 { print; exit }')"
  expected_install_name="@rpath/$framework_name.framework/$framework_name"
  if [[ "$install_name" != "$expected_install_name" ]]; then
    echo "Unexpected framework install name for $binary_path:" >&2
    echo "  actual:   $install_name" >&2
    echo "  expected: $expected_install_name" >&2
    return 1
  fi
}

vesper_ios_verify_foundation_bundle_metadata() {
  local framework_dir="$1"
  local framework_name="$2"
  local bundle_identifier="$3"
  local module_cache_dir="${VESPER_SWIFT_MODULE_CACHE_DIR:-${TMPDIR:-/tmp}/vesper-swift-module-cache}"

  vesper_require_command swift
  mkdir -p "$module_cache_dir"
  env \
    CLANG_MODULE_CACHE_PATH="$module_cache_dir" \
    SWIFT_MODULECACHE_PATH="$module_cache_dir" \
    swift -e '
    import Darwin
    import Foundation

    func fail(_ message: String) -> Never {
        FileHandle.standardError.write(Data((message + "\n").utf8))
        exit(1)
    }

    let frameworkPath = CommandLine.arguments[1]
    let expectedExecutable = CommandLine.arguments[2]
    let expectedIdentifier = CommandLine.arguments[3]
    guard let bundle = Bundle(path: frameworkPath),
          let info = bundle.infoDictionary,
          !info.isEmpty else {
        fail("Foundation could not read framework metadata: \(frameworkPath)")
    }
    guard bundle.bundleIdentifier == expectedIdentifier else {
        fail("Foundation read an unexpected bundle identifier: \(frameworkPath)")
    }
    guard info["CFBundleExecutable"] as? String == expectedExecutable,
          info["CFBundlePackageType"] as? String == "FMWK" else {
        fail("Foundation read invalid framework metadata: \(frameworkPath)")
    }
  ' "$framework_dir" "$framework_name" "$bundle_identifier"
}

vesper_ios_verify_framework_platform() {
  local framework_dir="$1"
  local framework_name="$2"
  local expected_bundle_platform="$3"
  local expected_sdk_name
  local expected_macho_platform
  local actual_bundle_platform
  local actual_sdk_name
  local build_metadata
  local actual_macho_platform

  case "$expected_bundle_platform" in
    iPhoneOS)
      expected_sdk_name="iphoneos"
      expected_macho_platform="IOS"
      ;;
    iPhoneSimulator)
      expected_sdk_name="iphonesimulator"
      expected_macho_platform="IOSSIMULATOR"
      ;;
    *)
      echo "Unsupported expected iOS framework platform: $expected_bundle_platform" >&2
      return 1
      ;;
  esac

  actual_bundle_platform="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleSupportedPlatforms:0' "$framework_dir/Info.plist" 2>/dev/null || true)"
  actual_sdk_name="$(/usr/libexec/PlistBuddy -c 'Print :DTPlatformName' "$framework_dir/Info.plist" 2>/dev/null || true)"
  if [[ "$actual_bundle_platform" != "$expected_bundle_platform" || "$actual_sdk_name" != "$expected_sdk_name" ]]; then
    echo "Unexpected framework bundle platform metadata: $framework_dir" >&2
    echo "  CFBundleSupportedPlatforms[0]: ${actual_bundle_platform:-<missing>} (expected $expected_bundle_platform)" >&2
    echo "  DTPlatformName: ${actual_sdk_name:-<missing>} (expected $expected_sdk_name)" >&2
    return 1
  fi

  vesper_require_command xcrun
  if ! build_metadata="$(xcrun vtool -show-build "$framework_dir/$framework_name" 2>&1)"; then
    echo "Unable to read Mach-O build platform: $framework_dir/$framework_name" >&2
    echo "$build_metadata" >&2
    return 1
  fi
  actual_macho_platform="$(
    printf '%s\n' "$build_metadata" | awk '
      $1 == "platform" && platform == "" { platform = $2 }
      $1 == "cmd" && $2 == "LC_VERSION_MIN_IPHONEOS" { legacy_ios = 1 }
      END {
        if (platform != "") {
          print platform
        } else if (legacy_ios) {
          print "IOS"
        }
      }
    '
  )"
  if [[ "$actual_macho_platform" != "$expected_macho_platform" ]]; then
    echo "Unexpected framework Mach-O build platform: $framework_dir/$framework_name" >&2
    echo "  actual:   ${actual_macho_platform:-<missing>}" >&2
    echo "  expected: $expected_macho_platform" >&2
    return 1
  fi
}

vesper_ios_verify_xcframework_manifest() {
  local xcframework_path="$1"
  local framework_name="$2"
  local manifest_path="$xcframework_path/Info.plist"
  local manifest_json
  local unexpected_symlink

  if [[ ! -f "$manifest_path" ]]; then
    echo "Missing XCFramework manifest: $manifest_path" >&2
    return 1
  fi
  unexpected_symlink="$(find "$xcframework_path" -type l -print -quit)"
  if [[ -n "$unexpected_symlink" ]]; then
    echo "XCFramework release payloads must not contain symlinks:" >&2
    echo "  $unexpected_symlink" >&2
    return 1
  fi
  vesper_require_command plutil
  vesper_require_command ruby
  if ! manifest_json="$(plutil -convert json -o - "$manifest_path")"; then
    echo "Unable to parse XCFramework manifest: $manifest_path" >&2
    return 1
  fi

  printf '%s\n' "$manifest_json" | ruby -rjson -e '
    manifest_path, xcframework_path, framework_name = ARGV
    manifest = JSON.parse(STDIN.read)
    abort "Unexpected XCFramework package type: #{manifest_path}" unless
      manifest["CFBundlePackageType"] == "XFWK"
    abort "Unexpected XCFramework format version: #{manifest_path}" unless
      manifest["XCFrameworkFormatVersion"] == "1.0"

    libraries = manifest["AvailableLibraries"]
    abort "XCFramework manifest must declare exactly two libraries: #{manifest_path}" unless
      libraries.is_a?(Array) && libraries.length == 2

    expected = {
      "ios-arm64" => nil,
      "ios-arm64-simulator" => "simulator",
    }
    expected_root_entries = ["Info.plist", *expected.keys].sort
    actual_root_entries = Dir.children(xcframework_path).sort
    unless actual_root_entries == expected_root_entries
      abort "Unexpected XCFramework top-level payload: #{actual_root_entries.inspect}"
    end
    libraries.each do |library|
      identifier = library["LibraryIdentifier"]
      abort "Unexpected XCFramework library identifier: #{identifier.inspect}" unless
        expected.key?(identifier)
      expected_variant = expected.delete(identifier)
      actual_variant = library["SupportedPlatformVariant"]
      unless actual_variant == expected_variant
        abort "Unexpected XCFramework SupportedPlatformVariant for #{identifier}: #{actual_variant.inspect}"
      end
      abort "Unexpected XCFramework platform for #{identifier}" unless
        library["SupportedPlatform"] == "ios"
      abort "Unexpected XCFramework architectures for #{identifier}" unless
        library["SupportedArchitectures"] == ["arm64"]
      abort "Unexpected XCFramework LibraryPath for #{identifier}" unless
        library["LibraryPath"] == "#{framework_name}.framework"
      abort "Unexpected XCFramework BinaryPath for #{identifier}" unless
        library["BinaryPath"] == "#{framework_name}.framework/#{framework_name}"

      library_path = File.join(xcframework_path, identifier, library["LibraryPath"])
      abort "XCFramework manifest LibraryPath does not exist for #{identifier}: #{library_path}" unless
        File.directory?(library_path)
      binary_path = File.join(xcframework_path, identifier, library["BinaryPath"])
      abort "XCFramework manifest BinaryPath does not exist for #{identifier}: #{binary_path}" unless
        File.file?(binary_path)
      slice_path = File.join(xcframework_path, identifier)
      actual_slice_entries = Dir.children(slice_path)
      unless actual_slice_entries == [library["LibraryPath"]]
        abort "Unexpected XCFramework slice payload for #{identifier}: #{actual_slice_entries.inspect}"
      end
    end
    abort "Missing XCFramework library identifiers: #{expected.keys.join(", ")}" unless expected.empty?
  ' "$manifest_path" "$xcframework_path" "$framework_name"
}

vesper_ios_verify_sibling_framework_dependencies() {
  local binary_path="$1"
  local sibling_root="$2"
  local dependency
  local framework_bundle

  while IFS= read -r dependency; do
    case "$dependency" in
      @rpath/VesperFFmpeg*.framework/*)
        framework_bundle="${dependency#@rpath/}"
        framework_bundle="${framework_bundle%%/*}"
        if [[ ! -d "$sibling_root/$framework_bundle" ]]; then
          echo "Missing sibling FFmpeg framework required by $binary_path:" >&2
          echo "  $sibling_root/$framework_bundle" >&2
          return 1
        fi
        ;;
    esac
  done < <(vesper_ios_binary_dependencies "$binary_path")
}

vesper_ios_verify_flat_framework() {
  local framework_dir="$1"
  local framework_name="$2"
  local package_type
  local executable_name
  local bundle_identifier
  local bundle_name
  local bundle_version
  local short_version
  local minimum_os_version
  local nested_code

  if [[ ! -f "$framework_dir/Info.plist" ]]; then
    echo "Missing framework Info.plist: $framework_dir/Info.plist" >&2
    return 1
  fi
  if [[ ! -f "$framework_dir/$framework_name" ]]; then
    echo "Missing framework executable: $framework_dir/$framework_name" >&2
    return 1
  fi

  package_type="$(/usr/libexec/PlistBuddy -c 'Print :CFBundlePackageType' "$framework_dir/Info.plist" 2>/dev/null || true)"
  executable_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$framework_dir/Info.plist" 2>/dev/null || true)"
  bundle_identifier="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$framework_dir/Info.plist" 2>/dev/null || true)"
  bundle_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleName' "$framework_dir/Info.plist" 2>/dev/null || true)"
  bundle_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$framework_dir/Info.plist" 2>/dev/null || true)"
  short_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$framework_dir/Info.plist" 2>/dev/null || true)"
  minimum_os_version="$(/usr/libexec/PlistBuddy -c 'Print :MinimumOSVersion' "$framework_dir/Info.plist" 2>/dev/null || true)"
  if [[ "$package_type" != "FMWK" || "$executable_name" != "$framework_name" ]]; then
    echo "Invalid framework bundle metadata: $framework_dir" >&2
    return 1
  fi
  if [[ -z "$bundle_identifier" || -z "$bundle_name" || -z "$bundle_version" || -z "$short_version" || -z "$minimum_os_version" ]]; then
    echo "Incomplete framework bundle metadata: $framework_dir/Info.plist" >&2
    return 1
  fi

  if [[ -e "$framework_dir/Resources" ]]; then
    echo "iOS shallow frameworks must keep resources at the framework root:" >&2
    echo "  $framework_dir/Resources" >&2
    echo "A top-level Resources directory makes Foundation ignore the root Info.plist." >&2
    return 1
  fi

  nested_code="$(find "$framework_dir" -mindepth 1 \
    \( -type d -name Frameworks -o -type d -name '*.framework' -o -type f -name '*.dylib*' \) \
    -print -quit)"
  if [[ -n "$nested_code" ]]; then
    echo "iOS frameworks must not contain nested frameworks or standalone dylibs:" >&2
    echo "  $nested_code" >&2
    return 1
  fi

  plutil -lint "$framework_dir/Info.plist" >/dev/null
  vesper_ios_verify_foundation_bundle_metadata \
    "$framework_dir" \
    "$framework_name" \
    "$bundle_identifier"
  vesper_ios_verify_framework_install_name "$framework_dir/$framework_name" "$framework_name"
  vesper_ios_verify_framework_binary_dependencies "$framework_dir/$framework_name"
}

vesper_ios_xcframework_slice_framework() {
  local xcframework_path="$1"
  local framework_name="$2"
  local platform_name="$3"
  local candidate

  while IFS= read -r candidate; do
    case "$platform_name" in
      iphoneos)
        if [[ "$candidate" != *simulator* ]]; then
          echo "$candidate"
          return 0
        fi
        ;;
      iphonesimulator)
        if [[ "$candidate" == *simulator* ]]; then
          echo "$candidate"
          return 0
        fi
        ;;
    esac
  done < <(find "$xcframework_path" -type d -name "$framework_name.framework" | sort)

  echo "Unable to find $platform_name slice for $framework_name in $xcframework_path" >&2
  return 1
}
