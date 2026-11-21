#!/usr/bin/env python3
"""Verify staged preview HARs and optional Release consumer native bytes."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tarfile
import zipfile


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--hap', type=Path)
    parser.add_argument('--no-internet', action='store_true')
    options = parser.parse_args()
    spec = importlib.util.spec_from_file_location('harmony_build', Path(__file__).with_name('build-kit.py'))
    build = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(build)
    report = json.loads((options.directory / 'build-report.json').read_text())
    if report['status'] != 'PASS':
        raise ValueError('The staged build did not pass')
    artifacts = {}
    native = report['artifacts']['vesper-player-kit']['nativeSha256']
    version = None
    for name in build.MODULES:
        path = options.directory / f'{name}.har'
        artifacts[name] = build.verify_har(path, native if name == build.MODULES[0] else {})
        if artifacts[name]['sha256'] != report['artifacts'][name]['sha256']:
            raise ValueError('HAR identity differs from the build report')
        with tarfile.open(path, 'r:gz') as archive:
            names = set(archive.getnames())
            if 'package/LICENSE' not in names:
                raise ValueError('HAR has no Vesper license')
            package = json.load(archive.extractfile('package/oh-package.json5'))
            if version is None:
                version = package['version']
            if package['version'] != version:
                raise ValueError('HAR versions differ')
            expected = {'@vesper/player-kit': version} if name == build.MODULES[1] else {'@vesper/player-kit-arkui': version} if name == build.MODULES[2] else {'libvesper_player_harmony_node.so': 'file:./src/main/cpp/types/libvesper_player_harmony_node'}
            if package['dependencies'] != expected:
                raise ValueError('HAR dependency direction or distribution reference is invalid')
            if name == build.MODULES[0] and 'package/NOTICE-libcxx.txt' not in names:
                raise ValueError('C++ runtime notice is missing')
    hap = None
    if options.hap:
        with zipfile.ZipFile(options.hap) as archive:
            if len(archive.infolist()) > 4096 or sum(item.file_size for item in archive.infolist()) > 256 * 1024 * 1024:
                raise ValueError('HAP expansion capacity exceeded')
            names = archive.namelist()
            if len(set(names)) != len(names) or any(Path(name).is_absolute() or '..' in Path(name).parts for name in names):
                raise ValueError('Invalid HAP paths')
            module = json.loads(archive.read('module.json'))
            app = module['app']
            if (app['minAPIVersion'] != build.PACKED_MIN_API_VERSION
                    or app['buildMode'] != 'release' or app['debug'] is not False):
                raise ValueError(f'Consumer is not Release with the API {build.MIN_API_VERSION} floor')
            libraries = {Path(name).name: hashlib.sha256(archive.read(name)).hexdigest() for name in names if name.endswith('.so')}
            if libraries != native or any('/arm64-v8a/' not in name for name in names if name.endswith('.so')):
                raise ValueError('Consumer native bytes differ from core HAR')
            permissions = [item['name'] for item in module['module'].get('requestPermissions', [])]
            if options.no_internet and 'ohos.permission.INTERNET' in permissions:
                raise ValueError('Offline consumer still requests INTERNET')
            hap = {'sha256': digest(options.hap), 'bundle': module['app']['bundleName'], 'permissions': permissions, 'nativeSha256': libraries}
    print(json.dumps({'schemaVersion': 1, 'status': 'PASS', 'scope': 'staged-harmony-artifact-contract',
                      'version': version, 'artifacts': artifacts, 'consumer': hap,
                      'unverified': ['Registry publication', 'Production signing', 'Physical devices']}, indent=2))


if __name__ == '__main__':
    main()
