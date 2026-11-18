#!/usr/bin/env python3
"""Run real native ABI and transpiled host lifecycle regression checks on macOS."""
import argparse
import importlib.util
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--tools', type=Path, required=True)
options = parser.parse_args()
root = Path(__file__).resolve().parents[2]
tools = options.tools.resolve()
subprocess.run(['cargo', 'build', '--locked', '--offline', '-p', 'player-ffi-harmony'], cwd=root, check=True, timeout=300, stdout=subprocess.DEVNULL)
output = root / 'target/harmony-kit/host'
output.mkdir(parents=True, exist_ok=True)
spec = importlib.util.spec_from_file_location('harmony_fixture', root / 'scripts/harmony/fixture-server.py')
fixture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fixture)
fixture.webvtt_mp4(output / 'captions.mp4')
module = output / 'vesper_player_harmony_node.node'
subprocess.run(['c++', '-std=c++17', '-Wall', '-Wextra', '-Werror', '-fno-exceptions', '-fno-rtti',
    '-DVESPER_HARMONY_HOST_TEST', '-DNAPI_VERSION=8', '-bundle', '-undefined', 'dynamic_lookup',
    '-I', str(tools / 'tool/node/include/node'), '-I', str(root / 'include'),
    str(root / 'lib/harmony/vesper-player-kit/src/main/cpp/native_module.cpp'),
    str(root / 'target/debug/libvesper_player_harmony.dylib'), '-o', str(module)], check=True, timeout=60)
environment = os.environ.copy()
environment['VESPER_HARMONY_HOST_MODULE'] = str(module)
environment['VESPER_HARMONY_TYPESCRIPT'] = str(tools / 'hvigor/hvigor-ohos-plugin/node_modules/typescript/lib/typescript.js')
environment['VESPER_WVTT_FIXTURE'] = str(output / 'captions.mp4')
subprocess.run([str(tools / 'tool/node/bin/node'), '--expose-gc', str(root / 'lib/harmony/tests/source-session.cjs')],
               cwd=root, env=environment, check=True, timeout=60)
subprocess.run([str(tools / 'tool/node/bin/node'), str(root / 'lib/harmony/tests/downloads.cjs')],
               cwd=root, env=environment, check=True, timeout=60)
subprocess.run([str(tools / 'tool/node/bin/node'), str(root / 'lib/harmony/tests/system-session.cjs')],
               cwd=root, env=environment, check=True, timeout=60)
subprocess.run([str(tools / 'tool/node/bin/node'), str(root / 'lib/harmony/tests/picture-in-picture.cjs')],
               cwd=root, env=environment, check=True, timeout=60)
subprocess.run([str(tools / 'tool/node/bin/node'), str(root / 'lib/harmony/tests/gestures.cjs')],
               cwd=root, env=environment, check=True, timeout=60)
subprocess.run([str(tools / 'tool/node/bin/node'), str(root / 'lib/harmony/tests/interaction.cjs')],
               cwd=root, env=environment, check=True, timeout=60)
subprocess.run([str(tools / 'tool/node/bin/node'), str(root / 'lib/harmony/tests/binding.cjs')],
               cwd=root, env=environment, check=True, timeout=60)
for suite in ['support', 'model', 'fullscreen', 'diagnostic-scenarios']:
    subprocess.run([str(tools / 'tool/node/bin/node'), str(root / f'examples/harmony-arkui-host/tests/{suite}.cjs')],
                   cwd=root, env=environment, check=True, timeout=60)
for suite in ['dvr-target', 'native-player-state']:
    subprocess.run([str(tools / 'tool/node/bin/node'), str(root / f'fixtures/harmony/kit-consumer/tests/{suite}.cjs')],
                   cwd=root, env=environment, check=True, timeout=60)
for suite in ['playback-view', 'window-policy', 'recovery', 'track-preferences', 'export-stream', 'stage-lifecycle', 'controller-commands', 'diagnostics', 'diagnostic-report', 'decoder-capabilities', 'dash-live']:
    subprocess.run([str(tools / 'tool/node/bin/node'), str(root / f'lib/harmony/tests/{suite}.cjs')],
                   cwd=root, env=environment, check=True, timeout=60)
subprocess.run([str(tools / 'tool/node/bin/node'), str(root / 'lib/harmony/tests/resource-server.cjs')],
               cwd=root, env=environment, check=True, timeout=60)
subprocess.run(['python3', '-m', 'unittest', 'discover', '-s', 'scripts/harmony/tests', '-v'],
               cwd=root, check=True, timeout=60)
