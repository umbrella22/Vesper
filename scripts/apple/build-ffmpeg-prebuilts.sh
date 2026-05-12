#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
FFMPEG_VERSION="${VESPER_APPLE_FFMPEG_VERSION:-8.1}"
FFMPEG_ARCHIVE_NAME="$(vesper_ffmpeg_archive_name "$FFMPEG_VERSION")"
FFMPEG_SOURCE_URL="${VESPER_APPLE_FFMPEG_SOURCE_URL:-$(vesper_ffmpeg_release_url "$FFMPEG_ARCHIVE_NAME")}"
FFMPEG_SOURCE_ARCHIVE="${VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE:-$ROOT_DIR/${FFMPEG_ARCHIVE_NAME}}"
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

selected_slices=()
while IFS= read -r slice; do
  selected_slices+=("$slice")
done < <(vesper_apple_resolve_selected_slices ${VESPER_FFMPEG_POSITIONAL_ARGS[@]+"${VESPER_FFMPEG_POSITIONAL_ARGS[@]}"})

vesper_require_command tar
vesper_require_command make
vesper_require_command xcrun

vesper_download_if_missing "$FFMPEG_SOURCE_ARCHIVE" "$FFMPEG_SOURCE_URL"

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
  metadata_expected="$WORK_DIR/metadata-$slice.txt"
  local_pkg_config_paths=()

  rm -rf "$pkgconfig_dir"
  mkdir -p "$pkgconfig_dir"

  if [[ "$VESPER_FFMPEG_USE_LIBXML2" == "1" ]]; then
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

  if [[ "$VESPER_FFMPEG_FORCE" != "1" && -f "$metadata_path" && -f "$output_root/lib/$output_libdir/libavformat.a" ]] && cmp -s "$metadata_path" "$metadata_expected"; then
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
