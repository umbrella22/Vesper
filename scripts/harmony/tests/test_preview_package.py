"""Portable consumer sources and archive hashes remain bound to their build receipts."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from consumer_project import project_files, ROOT_FILES, SOURCE_DIRECTORIES
from package_preview import MODULES, verify_example_project, verify_bundle


class PreviewPackageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.project = self.root / 'target/harmony-kit/consumer-fixture'
        for relative in ROOT_FILES:
            path = self.project / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('{}')
        for relative in SOURCE_DIRECTORIES:
            (self.project / relative).mkdir(parents=True, exist_ok=True)
        dependencies = {f'@vesper/{name.removeprefix("vesper-")}': f'file:../vendor/{name}.har' for name in MODULES}
        (self.project / 'entry/oh-package.json5').write_text(json.dumps({'dependencies': dependencies}))
        (self.project / 'oh-package.json5').write_text(json.dumps({'overrides': {
            name: path.replace('file:../', 'file:./') for name, path in dependencies.items()}}))
        for name in MODULES:
            (self.project / f'vendor/{name}.har').write_bytes(b'har-fixture')
        self.hashes = {f'{name}.har': hashlib.sha256(b'har-fixture').hexdigest() for name in MODULES}
        self.receipt = {'projectDirectory': str(self.project), 'projectFiles': project_files(self.project)}

    def test_portable_consumer_keeps_local_dependencies_and_excludes_caches(self):
        for name in ['node_modules/cache', '.hvigor/cache', 'entry/build/cache', 'build.log']:
            path = self.project / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('machine-local')
        self.assertEqual(verify_example_project(self.root, self.receipt, self.hashes), self.project.resolve())
        self.assertEqual(project_files(self.project), self.receipt['projectFiles'])

    def test_modified_sources_and_swapped_vendor_hars_are_rejected(self):
        path = self.project / 'entry/hvigorfile.ts'
        path.write_text('modified')
        with self.assertRaisesRegex(ValueError, 'source changed'):
            verify_example_project(self.root, self.receipt, self.hashes)
        path.write_text('{}')
        (self.project / f'vendor/{MODULES[0]}.har').write_bytes(b'other-har')
        self.receipt['projectFiles'] = project_files(self.project)
        with self.assertRaisesRegex(ValueError, 'vendor HAR differs'):
            verify_example_project(self.root, self.receipt, self.hashes)

    def test_registry_dependency_cannot_replace_vendored_resolution(self):
        path = self.project / 'entry/oh-package.json5'
        package = json.loads(path.read_text())
        package['dependencies']['@vesper/player-kit'] = '0.9.1-harmony.0'
        path.write_text(json.dumps(package))
        self.receipt['projectFiles'] = project_files(self.project)
        with self.assertRaisesRegex(ValueError, 'local HARs'):
            verify_example_project(self.root, self.receipt, self.hashes)

    def test_missing_source_and_external_links_are_rejected(self):
        path = self.project / 'entry/hvigorfile.ts'
        path.unlink()
        with self.assertRaises(FileNotFoundError):
            project_files(self.project)
        outside = self.root / 'external.ts'
        outside.write_text('external')
        path.symlink_to(outside)
        with self.assertRaisesRegex(ValueError, 'external links'):
            project_files(self.project)

    def test_archive_verification_rejects_tampering_and_unlisted_files(self):
        archive = self.root / 'preview.zip'
        sums = hashlib.sha256(b'payload').hexdigest() + '  example.hap\n'
        with zipfile.ZipFile(archive, 'w') as output:
            output.writestr('preview/example.hap', b'payload')
            output.writestr('preview/SHA256SUMS', sums)
        verify_bundle(archive)
        with zipfile.ZipFile(archive, 'w') as output:
            output.writestr('preview/example.hap', b'changed')
            output.writestr('preview/SHA256SUMS', sums)
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            verify_bundle(archive)
        with zipfile.ZipFile(archive, 'a') as output:
            output.writestr('preview/unlisted', 'unexpected')
        with self.assertRaisesRegex(ValueError, 'omits archive entries'):
            verify_bundle(archive)


if __name__ == '__main__':
    unittest.main()
