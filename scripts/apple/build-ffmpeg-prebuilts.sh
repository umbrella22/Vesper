#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg-validate.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
FFMPEG_VERSION="$(vesper_ffmpeg_resolve_version apple)"
FFMPEG_ARCHIVE_NAME="$(vesper_ffmpeg_archive_name "$FFMPEG_VERSION")"
FFMPEG_SOURCE_URL="${VESPER_APPLE_FFMPEG_SOURCE_URL:-$(vesper_ffmpeg_release_url "$FFMPEG_ARCHIVE_NAME")}"
FFMPEG_SOURCE_ARCHIVE="${VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE:-$(vesper_ffmpeg_source_cache_path "$FFMPEG_ARCHIVE_NAME")}"
FFMPEG_BASE_OUTPUT_DIR="$ROOT_DIR/third_party/ffmpeg/apple"
IOS_DEPLOYMENT_TARGET="$(vesper_apple_ios_deployment_target)"

vesper_ffmpeg_parse_common_args apple "$@"
FFMPEG_OUTPUT_DIR="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-${VESPER_FFMPEG_OUTPUT_DIR:-$(vesper_ffmpeg_default_output_dir apple "$FFMPEG_BASE_OUTPUT_DIR")}}"

apple_pkg_config_path() {
  local local_paths="$1"
  local existing="${PKG_CONFIG_PATH:-}"

  if [[ -n "$local_paths" && -n "$existing" ]]; then
    printf '%s:%s\n' "$local_paths" "$existing"
  elif [[ -n "$local_paths" ]]; then
    printf '%s\n' "$local_paths"
  else
    printf '%s\n' "$existing"
  fi
}

apple_pkg_config_command() {
  local candidate

  if [[ -n "${PKG_CONFIG:-}" ]]; then
    printf '%s\n' "$PKG_CONFIG"
    return 0
  fi

  if candidate="$(command -v pkg-config 2>/dev/null)"; then
    printf '%s\n' "$candidate"
    return 0
  fi

  for candidate in /opt/homebrew/bin/pkg-config /usr/local/bin/pkg-config; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  return 1
}

selected_slices=()
while IFS= read -r slice; do
  selected_slices+=("$slice")
done < <(vesper_apple_resolve_selected_slices ${VESPER_FFMPEG_POSITIONAL_ARGS[@]+"${VESPER_FFMPEG_POSITIONAL_ARGS[@]}"})

vesper_require_command tar
vesper_require_command make
vesper_require_command xcrun

mkdir -p "$(vesper_ffmpeg_source_cache_dir)"
vesper_download_if_missing "$FFMPEG_SOURCE_ARCHIVE" "$FFMPEG_SOURCE_URL"
if [[ -n "${VESPER_APPLE_FFMPEG_EXPECTED_SOURCE_SHA256:-}" ]]; then
  ACTUAL_SOURCE_SHA256="$(vesper_ffmpeg_sha256_file "$FFMPEG_SOURCE_ARCHIVE")"
  if [[ "$ACTUAL_SOURCE_SHA256" != "$VESPER_APPLE_FFMPEG_EXPECTED_SOURCE_SHA256" ]]; then
    echo "Apple FFmpeg source archive does not match the release-pinned SHA-256:" >&2
    echo "  expected: $VESPER_APPLE_FFMPEG_EXPECTED_SOURCE_SHA256" >&2
    echo "  actual:   $ACTUAL_SOURCE_SHA256" >&2
    exit 1
  fi
fi

MAKE_JOBS="$(vesper_make_jobs)"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vesper-apple-ffmpeg.XXXXXX")"
cleanup() {
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

mkdir -p "$FFMPEG_OUTPUT_DIR"

for slice in "${selected_slices[@]}"; do
  sdk_name="$(vesper_apple_slice_sdk "$slice")"
  arch="$(vesper_apple_slice_arch "$slice")"
  clang_target="$(vesper_apple_slice_clang_target "$slice" "$IOS_DEPLOYMENT_TARGET")"
  output_root="$(vesper_apple_slice_output_root "$slice" "$FFMPEG_OUTPUT_DIR")"
  output_libdir="$(vesper_apple_slice_output_libdir "$slice")"
  sdk_path="$(xcrun --sdk "$sdk_name" --show-sdk-path)"
  cc_path="$(xcrun --sdk "$sdk_name" -f clang)"
  source_dir="$WORK_DIR/source-$slice"
  install_dir="$WORK_DIR/install-$slice"
  pkgconfig_dir="$WORK_DIR/pkgconfig-$slice"
  metadata_path="$output_root/vesper-ffmpeg-build-metadata.txt"
  library_checksums_path="$output_root/vesper-ffmpeg-library-sha256.txt"
  metadata_expected="$WORK_DIR/metadata-$slice.txt"
  local_pkg_config_paths=()
  pkg_config_command=""

  rm -rf "$pkgconfig_dir"
  mkdir -p "$pkgconfig_dir"

  if vesper_ffmpeg_has_flag --enable-libxml2 ${VESPER_FFMPEG_CONFIGURE_ARGS[@]+"${VESPER_FFMPEG_CONFIGURE_ARGS[@]}"}; then
    if ! pkg_config_command="$(apple_pkg_config_command)"; then
      echo "pkg-config is required to configure Apple FFmpeg with libxml2." >&2
      echo "Install pkg-config/pkgconf, or set PKG_CONFIG to its executable path." >&2
      exit 1
    fi
    libxml2_version="$(vesper_apple_extract_libxml2_version "$sdk_path")"
    cat >"$pkgconfig_dir/libxml-2.0.pc" <<EOF
prefix=$sdk_path/usr
exec_prefix=\${prefix}
libdir=$sdk_path/usr/lib
includedir=$sdk_path/usr/include

Name: libxml2
Description: Apple SDK libxml2
Version: ${libxml2_version:-2.0.0}
Libs: -L\${libdir} -lxml2 -lz
Cflags: -I\${includedir}/libxml2
EOF
    local_pkg_config_paths+=("$pkgconfig_dir")
  fi

  extra_cflags=(
    "-target $clang_target"
    "-isysroot $sdk_path"
    "-fPIC"
    "-I$sdk_path/usr/include"
  )
  extra_ldflags=(
    "-target $clang_target"
    "-isysroot $sdk_path"
    "-L$sdk_path/usr/lib"
    "-lz"
    "-Wl,-headerpad_max_install_names"
  )

  configure_args=(
    "--prefix=$install_dir"
    "--install-name-dir=@rpath"
    "--enable-cross-compile"
    "--target-os=darwin"
    "--arch=$arch"
    "--cc=$cc_path"
    "--sysroot=$sdk_path"
    "--disable-programs"
    "--disable-doc"
    "--disable-autodetect"
    "--enable-static"
    "--enable-shared"
    "--enable-pic"
    "--extra-cflags=${extra_cflags[*]}"
    "--extra-ldflags=${extra_ldflags[*]}"
    ${VESPER_FFMPEG_CONFIGURE_ARGS[@]+"${VESPER_FFMPEG_CONFIGURE_ARGS[@]}"}
  )

  if [[ -n "$pkg_config_command" ]]; then
    configure_args+=("--pkg-config=$pkg_config_command")
  fi

  if [[ "$arch" == "x86_64" ]]; then
    # iOS simulator x86_64 is more likely to hit inline assembly issues on Apple Silicon hosts.
    configure_args+=("--disable-asm")
  fi

  vesper_ffmpeg_metadata_text \
    apple \
    "$slice" \
    "$FFMPEG_VERSION" \
    "$FFMPEG_SOURCE_ARCHIVE" \
    "$FFMPEG_SOURCE_URL" \
    ./configure \
    "${configure_args[@]}" >"$metadata_expected"

  cached_libraries_match=true
  if [[ ! -f "$library_checksums_path" ]]; then
    cached_libraries_match=false
  else
    for library_name in ${VESPER_FFMPEG_FINAL_LIBRARIES[@]+"${VESPER_FFMPEG_FINAL_LIBRARIES[@]}"}; do
      cached_library_path="$output_root/lib/$output_libdir/lib$library_name.dylib"
      cached_library_sha256="$(
        vesper_ffmpeg_metadata_value "$library_checksums_path" "${library_name}_sha256" 2>/dev/null || true
      )"
      if [[ ! -f "$cached_library_path" || \
        -z "$cached_library_sha256" || \
        "$(vesper_ffmpeg_sha256_file "$cached_library_path")" != "$cached_library_sha256" ]]
      then
        cached_libraries_match=false
        break
      fi
    done
  fi
  if [[ "$VESPER_FFMPEG_FORCE" != "1" && \
    -f "$metadata_path" && \
    -f "$output_root/lib/$output_libdir/libavformat.a" && \
    "$cached_libraries_match" == "true" ]] && \
    cmp -s "$metadata_path" "$metadata_expected"
  then
      echo "Apple FFmpeg prebuilt for $slice is up to date for profile $VESPER_FFMPEG_PROFILE."
      continue
  fi

  rm -rf "$source_dir" "$install_dir"
  mkdir -p "$source_dir" "$install_dir"
  tar -xf "$FFMPEG_SOURCE_ARCHIVE" -C "$source_dir" --strip-components=1

  echo
  echo "Building Apple FFmpeg prebuilt for $slice"
  echo "  profile: $VESPER_FFMPEG_PROFILE"
  echo "  output: $output_root"
  (
    cd "$source_dir"
    if [[ ${#local_pkg_config_paths[@]} -gt 0 ]]; then
      pkg_config_path_value="$(IFS=:; echo "${local_pkg_config_paths[*]}")"
    else
      pkg_config_path_value=""
    fi
    env \
      PKG_CONFIG_ALLOW_CROSS=1 \
      ${pkg_config_command:+"PKG_CONFIG=$pkg_config_command"} \
      PKG_CONFIG_PATH="$(apple_pkg_config_path "$pkg_config_path_value")" \
      PKG_CONFIG_LIBDIR="$(apple_pkg_config_path "$pkg_config_path_value")" \
      ./configure "${configure_args[@]}"
    make -j"$MAKE_JOBS"
    make install
  )

  mkdir -p "$output_root/lib/$output_libdir"
  rm -rf "$output_root/lib/$output_libdir"
  mkdir -p "$output_root/lib/$output_libdir"
  cp "$install_dir"/lib/*.a "$output_root/lib/$output_libdir/"
  if compgen -G "$install_dir/lib/"'lib*.dylib*' >/dev/null; then
    cp -RP "$install_dir"/lib/lib*.dylib* "$output_root/lib/$output_libdir/"
  fi

  rm -rf "$output_root/include"
  cp -R "$install_dir/include" "$output_root/include"
  : >"$library_checksums_path"
  for library_name in ${VESPER_FFMPEG_FINAL_LIBRARIES[@]+"${VESPER_FFMPEG_FINAL_LIBRARIES[@]}"}; do
    installed_library_path="$output_root/lib/$output_libdir/lib$library_name.dylib"
    if [[ ! -f "$installed_library_path" ]]; then
      echo "Missing built Apple FFmpeg shared library for checksum recording: $installed_library_path" >&2
      exit 1
    fi
    printf '%s_sha256=%s\n' \
      "$library_name" \
      "$(vesper_ffmpeg_sha256_file "$installed_library_path")" \
      >>"$library_checksums_path"
  done
  cp "$metadata_expected" "$metadata_path"
done

echo
echo "Built Apple FFmpeg prebuilts into:"
echo "  $FFMPEG_OUTPUT_DIR"
echo "Using FFmpeg source archive:"
echo "  $FFMPEG_SOURCE_ARCHIVE"
echo "FFmpeg profile:"
echo "  $VESPER_FFMPEG_PROFILE"
echo "Selected slices:"
for slice in "${selected_slices[@]}"; do
  echo "  $slice"
done
