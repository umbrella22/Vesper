#!/usr/bin/env bash
set -euo pipefail

if [[ $# != 5 ]]; then
    echo 'Usage: run-ios-device.sh <device-udid> <development-team> <h264-aac.mp4> <color-filter.metallib> <iphoneos-frameworks-dir>' >&2
    exit 2
fi

device=$1
team=$2
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
kit="$root/lib/ios/VesperPlayerKit"
mkdir -p "$root/target/native-plugin-reference"
output="$(mktemp -d "$root/target/native-plugin-reference/ios-device.XXXXXX")"
echo "Device probe evidence: $output"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target/native-plugin-work}"

cp "$3" "$output/vesper-native-frame-smoke.mp4"
cp "$4" "$output/vesper-color-filter.metallib"
frameworks="$(cd "$5" && pwd)"
framework="$output/VesperFixedGainProbe.framework"
mkdir "$framework"
xcrun --sdk iphoneos clang++ -std=c++17 -Wall -Wextra -Werror -arch arm64 \
    -miphoneos-version-min=17.0 -dynamiclib -fvisibility=hidden -DVESPER_PLUGIN_BUILD \
    -I "$root/include" -install_name '@rpath/VesperFixedGainProbe.framework/VesperFixedGainProbe' \
    "$root/templates/vesper-plugin/cpp-cmake/gain.cpp" -o "$framework/VesperFixedGainProbe"
xcodegen dump --quiet --type json --spec "$kit/project.yml" --file "$output/project.json"
python3 - "$root" "$output" "$frameworks" <<'PY'
import hashlib
import json
from pathlib import Path
import plistlib
import sys

root, output, frameworks = map(Path, sys.argv[1:])
gain = output / 'VesperFixedGainProbe.framework'
plugin_id = 'dev.vesper.example.gain-cpp'
(gain / 'Info.plist').write_bytes(plistlib.dumps({
    'CFBundleExecutable': gain.stem,
    'CFBundleIdentifier': plugin_id,
    'CFBundleName': gain.stem,
    'CFBundlePackageType': 'FMWK',
    'CFBundleShortVersionString': '1.0',
    'CFBundleVersion': '1',
    'CFBundleSupportedPlatforms': ['iPhoneOS'],
    'MinimumOSVersion': '17.0',
}))
source = root / 'templates/vesper-plugin/cpp-cmake/gain.cpp'
(gain / 'vesper-plugin-registry.json').write_text(json.dumps({
    'schema_version': 1,
    'target': 'aarch64-apple-ios',
    'architecture': 'arm64',
    'minimum_os': '17.0',
    'artifacts': [{
        'plugin_id': plugin_id,
        'transport': 'native',
        'locator': {'kind': 'apple-framework', 'name': gain.stem, 'bundle_identifier': plugin_id},
        'integrity': {'kind': 'apple-code-signature', 'validation': 'same-team-as-host-or-simulator-ad-hoc'},
        'package': {'version': '1.0.0', 'publisher': 'dev.vesper.example',
                    'descriptor_sha256': hashlib.sha256(source.read_bytes()).hexdigest()},
        'capabilities': [{'interface_id': 'a6da8f51-e472-513c-9fcc-9a28b26ca46b',
                          'instance_id': f'{plugin_id}.audio', 'interface_major': 1, 'interface_minor': 0}],
    }],
}) + '\n')

spec_path = output / 'project.json'
spec = json.loads(spec_path.read_text())
spec['name'] = 'VesperNativePluginDeviceProbe'
host = spec['targets']['VesperPlayerKitDeviceTestHost']
host['sources'] = [
    {'path': str(root / 'scripts/native-plugin-probe/ios-device/SmokeApp.swift')},
    {'path': str(output / 'vesper-native-frame-smoke.mp4'), 'buildPhase': 'resources'},
    {'path': str(output / 'vesper-color-filter.metallib'), 'buildPhase': 'resources'},
]
for name in ['VesperPlayerDecoderVideoToolboxPlugin', 'VesperPlayerSourceNormalizerFfmpegPlugin',
             'VesperFFmpegAVCodec', 'VesperFFmpegAVFormat', 'VesperFFmpegAVUtil', gain.stem]:
    framework = gain if name == gain.stem else frameworks / f'{name}.framework'
    if not (framework / name).is_file():
        sys.exit(f'Missing iPhoneOS framework binary: {framework / name}')
    host['dependencies'].append({'framework': str(framework), 'embed': True, 'link': False, 'codeSign': True})
spec_path.write_text(json.dumps(spec) + '\n')
PY

if [[ "${VESPER_IOS_FFI_PREBUILT:-0}" != 1 ]]; then
    VESPER_BUILD_APPLE_CATALYST=0 "$root/scripts/vesper" ios ffi release >"$output/ffi.log" 2>&1
fi
xcodegen generate --quiet --spec "$output/project.json" --project "$kit" --project-root "$kit"
project="$kit/VesperNativePluginDeviceProbe.xcodeproj"
VESPER_IOS_FFI_PREBUILT=1 xcodebuild build -project "$project" \
    -scheme VesperPlayerKitDeviceTestHost -destination "platform=iOS,id=$device,arch=arm64" \
    -derivedDataPath "$output/build" DEVELOPMENT_TEAM="$team" CODE_SIGN_STYLE=Automatic \
    >"$output/build.log" 2>&1
app="$output/build/Build/Products/Debug-iphoneos/VesperPlayerKitDeviceTestHost.app"
xcrun devicectl device install app --device "$device" "$app" >"$output/install.log" 2>&1
python3 - "$device" "$app" "$output/playback.log" <<'PY'
import plistlib
from pathlib import Path
import subprocess
import sys
import time

device, app, log = sys.argv[1:]
with (Path(app) / 'Info.plist').open('rb') as info:
    identifier = plistlib.load(info)['CFBundleIdentifier']
with open(log, 'w') as output:
    child = subprocess.Popen(['xcrun', 'devicectl', 'device', 'process', 'launch',
                              '--device', device, '--terminate-existing', '--console', identifier],
                             stdout=output, stderr=subprocess.STDOUT)
    try:
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            result = Path(log).read_text(errors='replace')
            if 'VESPER NATIVE PLUGIN SMOKE PASS' in result:
                print('VESPER NATIVE PLUGIN SMOKE PASS')
                sys.exit(0)
            if 'VESPER NATIVE PLUGIN SMOKE FAILED' in result or child.poll() is not None:
                sys.exit(f'Physical probe failed. See {log}')
            time.sleep(0.5)
        sys.exit(f'Physical probe timed out. See {log}')
    finally:
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
PY
