"""Packaging regressions exercise the formal HAR/HAP verifiers with tiny archives."""
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location('harmony_build', SCRIPTS / 'build-kit.py')
build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.native = {name: f'native fixture {name}'.encode() for name in build.LIBRARIES}
        self.hashes = {name: hashlib.sha256(data).hexdigest() for name, data in self.native.items()}
        self.module = {'app': {'minAPIVersion': 260000026, 'buildMode': 'release',
                              'debug': False, 'bundleName': 'dev.vesper.harmony.test'},
                       'module': {'requestPermissions': []}}

    def har_files(self, index=0):
        version = '0.9.1-harmony.0'
        dependencies = ({'libvesper_player_harmony_node.so': 'file:./src/main/cpp/types/libvesper_player_harmony_node'}
                        if index == 0 else {'@vesper/player-kit': version}
                        if index == 1 else {'@vesper/player-kit-arkui': version})
        package = {'version': version, 'dependencies': dependencies, 'compatibleSdkVersion': 26,
                   'metadata': {'debug': False}}
        files = {'package/LICENSE': b'license', 'package/NOTICE-libcxx.txt': b'notice',
                 'package/oh-package.json5': json.dumps(package).encode(),
                 'package/src/main/module.json': json.dumps(self.module).encode()}
        if index == 0:
            files.update({f'package/libs/arm64-v8a/{name}': data for name, data in self.native.items()})
        return files

    def write_har(self, files, name='fixture.har', extra=None):
        path = self.directory / name
        with tarfile.open(path, 'w:gz') as archive:
            for entry, data in files.items():
                info = tarfile.TarInfo(entry)
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
            if extra is not None:
                archive.addfile(extra)
        return path

    def stage_hars(self):
        artifacts = {}
        for index, name in enumerate(build.MODULES):
            path = self.write_har(self.har_files(index), name + '.har')
            artifacts[name] = build.verify_har(path, self.hashes if index == 0 else {})
        (self.directory / 'build-report.json').write_text(json.dumps({'status': 'PASS', 'artifacts': artifacts}))

    def write_hap(self, module=None, native=None, prefix='libs/arm64-v8a/'):
        path = self.directory / 'consumer.hap'
        with zipfile.ZipFile(path, 'w') as archive:
            archive.writestr('module.json', json.dumps(self.module if module is None else module))
            for name, data in (self.native if native is None else native).items():
                archive.writestr(prefix + name, data)
        return path

    def verify_hap(self, path, *extra):
        return subprocess.run([sys.executable, str(SCRIPTS / 'verify-artifacts.py'),
                               '--directory', str(self.directory), '--hap', str(path), *extra],
                              capture_output=True, text=True, timeout=15)

    def test_har_retains_exact_native_bytes(self):
        report = build.verify_har(self.write_har(self.har_files()), self.hashes)
        self.assertEqual(report['nativeSha256'], self.hashes)

    def test_har_rejects_changed_or_missing_native_bytes(self):
        key = 'package/libs/arm64-v8a/' + build.LIBRARIES[0]
        for changed in (False, True):
            with self.subTest(changed=changed):
                files = self.har_files()
                if changed:
                    files[key] = b'replaced'
                else:
                    del files[key]
                with self.assertRaisesRegex(ValueError, 'native bytes differ'):
                    build.verify_har(self.write_har(files), self.hashes)

    def test_har_rejects_api_floor_and_debug_changes(self):
        for field, value in [('compatibleSdkVersion', 20), ('metadata', {'debug': True})]:
            with self.subTest(field=field):
                files = self.har_files()
                package = json.loads(files['package/oh-package.json5'])
                package[field] = value
                files['package/oh-package.json5'] = json.dumps(package).encode()
                with self.assertRaisesRegex(ValueError, 'API 26 and Release'):
                    build.verify_har(self.write_har(files), self.hashes)
        files = self.har_files()
        module = copy.deepcopy(self.module)
        module['app']['minAPIVersion'] = 60000020
        files['package/src/main/module.json'] = json.dumps(module).encode()
        with self.assertRaisesRegex(ValueError, 'packed API 26'):
            build.verify_har(self.write_har(files), self.hashes)

    def test_har_rejects_unsafe_archive_entries(self):
        for name, kind in [('../escape', tarfile.REGTYPE), ('package/LICENSE', tarfile.REGTYPE),
                           ('package/link', tarfile.SYMTYPE)]:
            with self.subTest(name=name):
                info = tarfile.TarInfo(name)
                info.type = kind
                info.linkname = 'outside'
                with self.assertRaisesRegex(ValueError, 'Invalid HAR'):
                    build.verify_har(self.write_har(self.har_files(), extra=info), self.hashes)

    def test_hap_retains_native_bytes_and_packed_runtime_floor(self):
        self.stage_hars()
        result = self.verify_hap(self.write_hap(), '--no-internet')
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report['consumer']['nativeSha256'], self.hashes)
        self.assertEqual(report['consumer']['permissions'], [])

    def test_hap_rejects_changed_missing_or_wrong_abi_native(self):
        self.stage_hars()
        for mode in ['changed', 'missing', 'abi']:
            with self.subTest(mode=mode):
                native = dict(self.native)
                if mode == 'changed':
                    native[build.LIBRARIES[0]] = b'replaced'
                if mode == 'missing':
                    del native[build.LIBRARIES[0]]
                path = self.write_hap(native=native, prefix='libs/x86_64/' if mode == 'abi' else 'libs/arm64-v8a/')
                result = self.verify_hap(path)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Consumer native bytes differ', result.stderr)

    def test_hap_rejects_debug_buildmode_and_os_api_changes(self):
        self.stage_hars()
        for field, value in [('minAPIVersion', 26), ('minAPIVersion', 60000020),
                             ('minAPIVersion', 260000025), ('debug', True), ('buildMode', 'debug')]:
            with self.subTest(field=field, value=value):
                module = copy.deepcopy(self.module)
                module['app'][field] = value
                result = self.verify_hap(self.write_hap(module=module))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('Release with the API 26 floor', result.stderr)

    def test_offline_hap_rejects_internet_permission(self):
        self.stage_hars()
        module = copy.deepcopy(self.module)
        module['module']['requestPermissions'] = [{'name': 'ohos.permission.INTERNET'}]
        result = self.verify_hap(self.write_hap(module=module), '--no-internet')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Offline consumer still requests INTERNET', result.stderr)


if __name__ == '__main__':
    unittest.main()
