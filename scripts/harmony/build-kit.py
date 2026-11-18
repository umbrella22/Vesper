#!/usr/bin/env python3
"""Build staged Harmony HARs with explicit tools and audited native dependencies."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
from candidate_identity import source_identity


MODULES = ['vesper-player-kit', 'vesper-player-kit-arkui', 'vesper-player-kit-arkui-ui']
TARGET = 'aarch64-unknown-linux-ohos'
LIBRARIES = ['libvesper_player_harmony.so', 'libvesper_player_harmony_node.so', 'libc++_shared.so']


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(arguments, cwd, environment, log, timeout=600):
    with log.open('ab') as output:
        result = subprocess.run([str(a) for a in arguments], cwd=cwd, env=environment,
                                stdout=output, stderr=subprocess.STDOUT, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f'Command failed ({result.returncode}); inspect {log}')


def verify_har(path, expected_native):
    if path.stat().st_size > 128 * 1024 * 1024:
        raise ValueError('HAR exceeds 128 MiB')
    names = set()
    expanded = 0
    native = {}
    package = None
    module = None
    with tarfile.open(path, 'r:gz') as archive:
        for entry in archive:
            if len(names) >= 4096 or entry.name in names or entry.issym() or entry.islnk():
                raise ValueError('Invalid HAR entries')
            relative = Path(entry.name)
            if relative.is_absolute() or '..' in relative.parts:
                raise ValueError('Invalid HAR path')
            names.add(entry.name)
            expanded += entry.size
            if entry.size > 32 * 1024 * 1024 or expanded > 256 * 1024 * 1024:
                raise ValueError('HAR expansion capacity')
            if entry.isfile() and entry.name.endswith('.so'):
                if '/libs/arm64-v8a/' not in entry.name or relative.name not in LIBRARIES:
                    raise ValueError(f'Unexpected native library: {entry.name}')
                native[relative.name] = hashlib.sha256(archive.extractfile(entry).read()).hexdigest()
            if entry.isfile() and entry.name == 'package/oh-package.json5':
                package = json.load(archive.extractfile(entry))
            if entry.isfile() and entry.name == 'package/src/main/module.json':
                module = json.load(archive.extractfile(entry))
    if native != expected_native:
        raise ValueError('HAR native bytes differ from their staged inputs')
    if package is None or package.get('compatibleSdkVersion') != 20 or package.get('metadata', {}).get('debug') is not False:
        raise ValueError('HAR must use compatible API 20 and Release metadata')
    app = module.get('app', {}) if module is not None else {}
    if app.get('minAPIVersion') != 60000020 or app.get('buildMode') != 'release' or app.get('debug') is not False:
        raise ValueError('HAR module must retain the packed API 20 floor and Release metadata')
    return {'sha256': digest(path), 'entries': len(names), 'nativeSha256': native}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tools', type=Path, required=True)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--offline', action='store_true')
    parser.add_argument('--core-only', action='store_true')
    options = parser.parse_args()
    root = options.root.resolve()
    tools = options.tools.resolve()
    identity = source_identity(root)
    native = tools / 'sdk/default/openharmony/native'
    base = root / 'target/harmony-kit'
    base.mkdir(parents=True, exist_ok=True)
    project = Path(tempfile.mkdtemp(prefix='build-', dir=base))
    log = project / 'build.log'
    environment = os.environ.copy()
    environment.update({
        'NODE_HOME': str(tools / 'tool/node'), 'DEVECO_NODE_HOME': str(tools / 'tool/node'),
        'DEVECO_SDK_HOME': str(tools / 'sdk'), 'OHOS_NDK_HOME': str(native),
        'RUSTUP_AUTO_INSTALL': '0', 'CARGO_TARGET_DIR': str(base),
        'CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER': str(native / 'llvm/bin/clang'),
        'CARGO_PROFILE_RELEASE_DEBUG': '2', 'CARGO_PROFILE_RELEASE_STRIP': 'none',
        'CARGO_ENCODED_RUSTFLAGS': '\x1f'.join(['-C', 'link-arg=--target=aarch64-linux-ohos',
            '-C', f'link-arg=--sysroot={native / "sysroot"}', '-C', 'link-arg=-Wl,--no-undefined']),
    })
    run(['cbindgen', root / 'crates/ffi/player-ffi-harmony', '--config',
         root / 'crates/ffi/player-ffi-harmony/cbindgen.toml', '--lockfile', root / 'Cargo.lock',
         '--output', root / 'include/vesper_harmony.h', '--only-target-dependencies', '--verify'],
        root, environment, log)
    cargo = ['cargo', 'build', '--locked', '--release', '--target', TARGET, '-p', 'player-ffi-harmony']
    if options.offline:
        cargo.append('--offline')
    run(cargo, root, environment, log)
    release = base / TARGET / 'release'
    run([native / 'llvm/bin/clang++', '--target=aarch64-linux-ohos',
         f'--sysroot={native / "sysroot"}', '-std=c++17', '-D__OHOS_API__=20', '-Wall', '-Wextra',
         '-Werror', '-fno-exceptions', '-fno-rtti', '-fPIC', '-shared', '-O2', '-g', '-I', root / 'include',
         root / 'lib/harmony/vesper-player-kit/src/main/cpp/native_module.cpp', '-L', release,
         '-lvesper_player_harmony', '-lace_napi.z', '-lnative_media_codecbase', '-Wl,--no-undefined', '-Wl,-rpath,$ORIGIN',
         '-Wl,-soname,libvesper_player_harmony_node.so', '-o', release / LIBRARIES[1]],
        root, environment, log, 60)
    source = root / 'lib/harmony'
    for name in ['oh-package.json5', 'build-profile.json5', 'hvigorfile.ts']:
        shutil.copy2(source / name, project / name)
    (project / 'hvigor').mkdir()
    (project / 'hvigor/hvigor-config.json5').write_text('{"modelVersion":"6.0.0","dependencies":{}}\n')
    (project / 'AppScope').mkdir()
    (project / 'AppScope/app.json5').write_text(json.dumps({'app': {
        'bundleName': 'dev.vesper.harmony.kit', 'vendor': 'Vesper',
        'versionCode': 1, 'versionName': '0.9.1-harmony.0'}}))
    modules = MODULES[:1] if options.core_only else MODULES
    config = json.loads((project / 'build-profile.json5').read_text())
    config['modules'] = [entry for entry in config['modules'] if Path(entry['srcPath']).name in modules]
    (project / 'build-profile.json5').write_text(json.dumps(config, indent=2))
    for name in modules:
        shutil.copytree(source / name, project / name,
                        ignore=shutil.ignore_patterns('build', 'oh_modules', '.hvigor', 'libs'))
        shutil.copy2(root / 'LICENSE', project / name / 'LICENSE')
    (project / MODULES[0] / 'src/main/ets/BuildIdentity.ets').write_text(
        '/** Source candidate identity recorded by the HAR builder. */\n'
        f'export const VESPER_HARMONY_VERSION: string = {json.dumps(json.loads((source / MODULES[0] / "oh-package.json5").read_text())["version"])};\n'
        f'export const VESPER_HARMONY_SOURCE_SHA256: string = {json.dumps(identity["sourceSha256"])};\n')
    libraries = project / MODULES[0] / 'libs/arm64-v8a'
    libraries.mkdir(parents=True)
    symbols = base / 'symbols'
    symbols.mkdir(exist_ok=True)
    dependencies = {}
    allowed = {'libc.so', 'libdl.so', 'libm.so', 'libpthread.so', 'librt.so',
               'libc++_shared.so', 'libace_napi.z.so', 'libvesper_player_harmony.so', 'libnative_media_codecbase.so'}
    import re
    for name in LIBRARIES:
        original = native / 'llvm/lib/aarch64-linux-ohos/libc++_shared.so' if name == LIBRARIES[2] else release / name
        shutil.copy2(original, libraries / name)
        if name != LIBRARIES[2]:
            run([native / 'llvm/bin/llvm-objcopy', '--only-keep-debug', original, symbols / f'{name}.debug'], root, environment, log, 60)
            run([native / 'llvm/bin/llvm-strip', '--strip-debug', libraries / name], root, environment, log, 60)
        elf = subprocess.check_output([native / 'llvm/bin/llvm-readelf', '-h', '-d', libraries / name], text=True, timeout=15)
        if 'AArch64' not in elf:
            raise ValueError('Native architecture is not arm64')
        needed = sorted(re.findall(r'\(NEEDED\).*?\[(.*?)\]', elf))
        if set(needed) - allowed:
            raise ValueError(f'Unapproved dynamic dependencies: {needed}')
        dependencies[name] = needed
    shutil.copy2(native / 'llvm/NOTICE', project / MODULES[0] / 'NOTICE-libcxx.txt')
    expected_native = {name: digest(libraries / name) for name in LIBRARIES}
    (project / 'node_modules/@ohos').mkdir(parents=True)
    for plugin in ['hvigor', 'hvigor-ohos-plugin']:
        (project / 'node_modules/@ohos' / plugin).symlink_to(tools / 'hvigor' / plugin, target_is_directory=True)
    run([tools / 'bin/ohpm', 'install', '--all'], project, environment, log, 120)
    artifacts = {}
    dist = base / 'dist'
    dist.mkdir(exist_ok=True)
    for name in modules:
        module_name = name.replace('-', '_')
        run([tools / 'bin/hvigorw', '--mode', 'module', '-p', f'module={module_name}@default', '-p',
             'product=default', '-p', 'buildMode=release', 'assembleHar', '--no-daemon'], project, environment, log, 300)
        output = project / name / 'build/default/outputs/default' / f'{name}.har'
        if not output.is_file():
            candidates = list((project / name / 'build/default/outputs/default').glob('*.har'))
            if len(candidates) != 1:
                raise RuntimeError('Expected exactly one HAR output')
            output = candidates[0]
        artifact = dist / f'{name}.har'
        shutil.copy2(output, artifact)
        artifacts[name] = verify_har(artifact, expected_native if name == MODULES[0] else {})
    if source_identity(root)['sourceSha256'] != identity['sourceSha256']:
        raise ValueError('Build inputs changed while HARs were being built; rebuild the candidate')
    report = {'schemaVersion': 1, 'scope': 'harmony-host-kit-release-build', 'status': 'PASS', 'source': identity,
              'projectDirectory': str(project), 'artifacts': artifacts, 'dynamicDependencies': dependencies,
              'lockfileSha256': digest(root / 'Cargo.lock'), 'symbols': str(symbols),
              'unverified': ['Runtime playback', 'Physical devices', 'Registry publication']}
    (dist / 'build-report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
