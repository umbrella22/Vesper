"""Candidate identity separates build inputs from editor state without relaxing validation."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from candidate_identity import digest, require_candidate, require_runtime_candidate, source_identity


class CandidateIdentityTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        subprocess.run(['git', 'init', '--quiet', str(self.root)], check=True, capture_output=True)
        subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                        'commit', '--quiet', '--allow-empty', '-m', 'fixture'],
                       cwd=self.root, check=True, capture_output=True)
        self.inputs = ['crates/sample/src/lib.rs', 'lib/harmony/build-profile.json5',
                       'examples/harmony-arkui-host/entry/src/main/resources/rawfile/sample.bin']
        self.editor = ['lib/harmony/.codeartsdoer/mem-exp-config.json',
                       'lib/harmony/.codeartsdoer/.codebase/index_records.json',
                       'lib/harmony/.merkle-snapshot.json']
        self.artifact = 'target/harmony-kit/dist/example.hap'
        self.media = 'target/harmony-kit/media/sample.mp4'
        for name in self.inputs + self.editor + [self.artifact, self.media]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'fixture')
        self.candidate = {'source': source_identity(self.root),
                          'artifacts': {self.artifact: digest(self.root / self.artifact)},
                          'media': {self.media: digest(self.root / self.media)}}
        self.manifest = self.root / 'target/candidate.json'
        self.manifest.write_text(json.dumps(self.candidate))

    def test_editor_state_changes_do_not_invalidate_build_inputs(self):
        self.assertEqual(set(self.candidate['source']['files']), set(self.inputs))
        for name in self.editor:
            (self.root / name).write_bytes(b'changed editor state')
        require_candidate(self.root, self.manifest)
        for name in self.editor:
            (self.root / name).unlink()
        (self.root / 'lib/harmony/.codeartsdoer/new-state.json').write_text('{}')
        require_candidate(self.root, self.manifest)

    def test_real_source_configuration_and_resources_remain_strict(self):
        for name in self.inputs:
            path = self.root / name
            for mode in ['changed', 'removed']:
                with self.subTest(name=name, mode=mode):
                    if mode == 'changed':
                        path.write_bytes(b'changed build input')
                    else:
                        path.unlink()
                    with self.assertRaisesRegex(ValueError, 'Working source changed'):
                        require_candidate(self.root, self.manifest)
                    path.write_bytes(b'fixture')
        (self.root / 'lib/harmony/new.ets').write_text('export const value = 1;')
        with self.assertRaisesRegex(ValueError, 'Working source changed'):
            require_candidate(self.root, self.manifest)

    def test_artifact_and_media_validation_is_unchanged(self):
        for name, message in [(self.artifact, 'Frozen artifact changed'), (self.media, 'Frozen media changed')]:
            with self.subTest(name=name):
                path = self.root / name
                path.write_bytes(b'changed payload')
                with self.assertRaisesRegex(ValueError, message):
                    require_candidate(self.root, self.manifest)
                path.write_bytes(b'fixture')

    def test_legacy_fingerprint_is_not_silently_reinterpreted(self):
        files = {**self.candidate['source']['files'], **{name: digest(self.root / name) for name in self.editor}}
        self.candidate['source']['files'] = files
        self.candidate['source']['sourceSha256'] = hashlib.sha256(
            json.dumps(files, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        self.manifest.write_text(json.dumps(self.candidate))
        with self.assertRaisesRegex(ValueError, 'Working source changed'):
            require_candidate(self.root, self.manifest)

    def test_build_only_candidate_is_rejected_before_runtime_acceptance(self):
        self.candidate['media'] = {}
        self.candidate['runtimeAcceptanceReady'] = False
        self.manifest.write_text(json.dumps(self.candidate))
        require_candidate(self.root, self.manifest)
        with self.assertRaisesRegex(ValueError, 'Build-only candidates cannot run final runtime acceptance'):
            require_runtime_candidate(self.root, self.manifest)

    def test_runtime_acceptance_requires_a_bound_media_catalog(self):
        with self.assertRaisesRegex(ValueError, 'prepare and freeze media first'):
            require_runtime_candidate(self.root, self.manifest)
        catalog = 'target/harmony-kit/media/catalog.json'
        (self.root / catalog).write_text('{}')
        self.candidate['media'][catalog] = digest(self.root / catalog)
        self.manifest.write_text(json.dumps(self.candidate))
        require_runtime_candidate(self.root, self.manifest)


if __name__ == '__main__':
    unittest.main()
