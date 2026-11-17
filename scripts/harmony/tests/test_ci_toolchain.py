"""SDK installation rejects corrupt archives and filesystem escapes before execution."""
import io
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from ci_toolchain import extract_verified, sha256


class ToolchainArchiveTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.archive = self.root / 'tools.tar.gz'
        self.destination = self.root / 'installed'

    def write_archive(self, name, link=None, mode=0o755):
        with tarfile.open(self.archive, 'w:gz') as archive:
            entry = tarfile.TarInfo(name)
            entry.mode = mode
            if link is not None:
                entry.type = tarfile.SYMTYPE
                entry.linkname = link
                archive.addfile(entry)
            else:
                entry.size = 4
                archive.addfile(entry, io.BytesIO(b'tool'))

    def test_checksum_failure_never_extracts(self):
        self.write_archive('command-line-tools/bin/tool')
        with self.assertRaisesRegex(ValueError, 'SHA-256 mismatch'):
            extract_verified(self.archive, '0' * 64, self.destination)
        self.assertFalse(self.destination.exists())

    def test_path_and_symlink_escapes_are_rejected(self):
        for name, link in [('command-line-tools/../../escaped', None),
                           ('/absolute', None), ('other-root/tool', None),
                           ('command-line-tools/bin/tool', '../../../escaped')]:
            with self.subTest(name=name, link=link):
                self.write_archive(name, link)
                with self.assertRaisesRegex(ValueError, 'Invalid SDK|escapes'):
                    extract_verified(self.archive, sha256(self.archive), self.destination)
                self.assertFalse(self.destination.exists())
                self.assertFalse((self.root / 'escaped').exists())

    def test_executability_and_relative_links_survive_installation(self):
        self.write_archive('command-line-tools/bin/clang-15')
        with tarfile.open(self.archive, 'r:gz') as original:
            entry = original.getmembers()[0]
            payload = original.extractfile(entry).read()
        with tarfile.open(self.archive, 'w:gz') as archive:
            archive.addfile(entry, io.BytesIO(payload))
            link = tarfile.TarInfo('command-line-tools/bin/clang')
            link.type = tarfile.SYMTYPE
            link.linkname = 'clang-15'
            archive.addfile(link)
        with patch('ci_toolchain.validate_tools', return_value={'sdk': 'fixture'}):
            extract_verified(self.archive, sha256(self.archive), self.destination)
        self.assertTrue((self.destination / 'bin/clang').is_symlink())
        self.assertEqual((self.destination / 'bin/clang').read_bytes(), b'tool')
        self.assertEqual((self.destination / 'bin/clang-15').stat().st_mode & 0o111, 0o111)

    def test_failed_validation_does_not_leave_an_installation(self):
        self.write_archive('command-line-tools/bin/tool')
        with self.assertRaises(FileNotFoundError):
            extract_verified(self.archive, sha256(self.archive), self.destination)
        self.assertFalse(self.destination.exists())


if __name__ == '__main__':
    unittest.main()
