"""Inventory the portable source of a generated local-HAR consumer."""
import hashlib
from pathlib import Path


ROOT_FILES = ('build-profile.json5', 'hvigorfile.ts', 'oh-package.json5',
              'entry/build-profile.json5', 'entry/hvigorfile.ts', 'entry/oh-package.json5')
SOURCE_DIRECTORIES = ('AppScope', 'entry/src', 'hvigor', 'vendor')


def project_files(project):
    project = Path(project).resolve()
    paths = [project / relative for relative in ROOT_FILES]
    for relative in SOURCE_DIRECTORIES:
        directory = project / relative
        if not directory.is_dir():
            raise FileNotFoundError(directory)
        paths.extend(path for path in directory.rglob('*') if path.is_file())
    files = {}
    for path in sorted(paths):
        if path.is_symlink() or not path.resolve().is_relative_to(project):
            raise ValueError('Portable consumer source must not contain external links')
        files[str(path.relative_to(project))] = hashlib.sha256(path.read_bytes()).hexdigest()
    return files
