#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
slang_version=2026.5.2
compiler="${SLANGC:-slangc}"
if [[ "$("$compiler" -version 2>&1)" != "$slang_version" ]]; then
    echo "Use Slang $slang_version for reference filter builds." >&2
    exit 4
fi
backend="${1:?Usage: plugin-shader-build.sh <metal|ios|ios-simulator|vulkan|d3d11|d3d11-hlsl> <output-directory>}"
output="${2:?An output directory is required}"
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
compile() {
    "$compiler" "$root/plugins/video-filter-reference/color-filter.slang" \
        -I "$root/plugins/common-shaders" -entry colorFilter -stage compute "$@"
}
case "$backend" in
    metal|ios|ios-simulator)
        case "$backend" in
            metal) sdk=macosx; minimum=-mmacosx-version-min=11.0 ;;
            ios) sdk=iphoneos; minimum=-mios-version-min=17.0 ;;
            ios-simulator) sdk=iphonesimulator; minimum=-mios-simulator-version-min=17.0 ;;
        esac
        compile -target metal -o "$output/color-filter.metal"
        xcrun --sdk "$sdk" metal "$minimum" -c "$output/color-filter.metal" -o "$output/color-filter.air"
        xcrun --sdk "$sdk" metallib "$output/color-filter.air" -o "$output/color-filter.metallib"
        ;;
    vulkan)
        compile -target spirv -profile spirv_1_3 -o "$output/color-filter.spv"
        compile -target spirv -profile spirv_1_3 -DVESPER_VULKAN_COMBINED_420=1 \
            -o "$output/color-filter-combined-420.spv"
        ;;
    d3d11|d3d11-hlsl)
        compile -target hlsl -profile cs_5_0 -DVESPER_D3D11_SDR_TARGET=1 -o "$output/color-filter.hlsl"
        if [[ "$backend" == d3d11 ]]; then
            fxc="${FXC:-fxc.exe}"
            if ! command -v "$fxc" >/dev/null; then
                echo 'Set FXC to the Windows SDK fxc.exe to produce cs_5_0 DXBC.' >&2
                exit 4
            fi
            input_path="$output/color-filter.hlsl"
            output_path="$output/color-filter.dxbc"
            if command -v cygpath >/dev/null; then
                input_path="$(cygpath -w "$input_path")"
                output_path="$(cygpath -w "$output_path")"
            fi
            # MSYS must pass the Windows-style compiler switches verbatim.
            MSYS2_ARG_CONV_EXCL='*' "$fxc" /nologo /WX /O3 /T cs_5_0 /E colorFilter \
                /Fo "$output_path" "$input_path"
        fi
        ;;
    *) echo "Unsupported shader backend: $backend" >&2; exit 2 ;;
esac
