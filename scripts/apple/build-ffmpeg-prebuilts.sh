#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/apple.sh"
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/../lib" && pwd)/ffmpeg.sh"

ROOT_DIR="$VESPER_REPO_ROOT"
FFMPEG_VERSION="${VESPER_APPLE_FFMPEG_VERSION:-8.1}"
FFMPEG_ARCHIVE_NAME="$(vesper_ffmpeg_archive_name "$FFMPEG_VERSION")"
FFMPEG_SOURCE_URL="${VESPER_APPLE_FFMPEG_SOURCE_URL:-$(vesper_ffmpeg_release_url "$FFMPEG_ARCHIVE_NAME")}"
FFMPEG_SOURCE_ARCHIVE="${VESPER_APPLE_FFMPEG_SOURCE_ARCHIVE:-$ROOT_DIR/${FFMPEG_ARCHIVE_NAME}}"
FFMPEG_OUTPUT_DIR="${VESPER_APPLE_FFMPEG_OUTPUT_DIR:-$ROOT_DIR/third_party/ffmpeg/apple}"
IOS_DEPLOYMENT_TARGET="$(vesper_apple_ios_deployment_target)"
ENABLE_DASH="${VESPER_APPLE_FFMPEG_ENABLE_DASH:-1}"

selected_slices=()
while IFS= read -r slice; do
  selected_slices+=("$slice")
done < <(vesper_apple_resolve_selected_slices "$@")

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
  libxml2_version="$(vesper_apple_extract_libxml2_version "$sdk_path")"
  source_dir="$WORK_DIR/source-$slice"
  install_dir="$WORK_DIR/install-$slice"
  pkgconfig_dir="$WORK_DIR/pkgconfig-$slice"

  rm -rf "$source_dir" "$install_dir" "$pkgconfig_dir"
  mkdir -p "$source_dir" "$install_dir" "$pkgconfig_dir"
  tar -xf "$FFMPEG_SOURCE_ARCHIVE" -C "$source_dir" --strip-components=1

  cat > "$pkgconfig_dir/libxml-2.0.pc" <<EOF
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
    "--enable-network"
    "--enable-securetransport"
    "--extra-cflags=${extra_cflags[*]}"
    "--extra-ldflags=${extra_ldflags[*]}"
  )

  if [[ "$ENABLE_DASH" == "1" ]]; then
    configure_args+=("--enable-libxml2")
  fi

  if [[ "$arch" == "x86_64" ]]; then
    # iOS simulator x86_64 is more likely to hit inline assembly issues on Apple Silicon hosts.
    configure_args+=("--disable-asm")
  fi

  echo
  echo "Building Apple FFmpeg prebuilt for $slice"
  (
    cd "$source_dir"
    env \
      PKG_CONFIG_ALLOW_CROSS=1 \
      PKG_CONFIG_PATH="$pkgconfig_dir" \
      PKG_CONFIG_LIBDIR="$pkgconfig_dir" \
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
done

echo
echo "Built Apple FFmpeg prebuilts into:"
echo "  $FFMPEG_OUTPUT_DIR"
echo "Selected slices:"
for slice in "${selected_slices[@]}"; do
  echo "  $slice"
done
